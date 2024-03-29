//! Single client connection

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::solicit::end_stream::EndStream;
use crate::solicit::header::*;
use crate::AnySocketAddr;
use crate::HttpScheme;

use tls_api::AsyncSocket;
use tls_api::TlsConnectorBox;

use tls_api;

use crate::solicit_async::*;

use crate::client::handler::ClientHandler;
use crate::client::intf::ClientInternals;
use crate::client::intf::ClientIntf;
use crate::client::types::ClientTypes;
use crate::common::conn::Conn;
use crate::common::conn::ConnStateSnapshot;
use crate::common::conn::SideSpecific;
use crate::common::conn_read::ConnReadSideCustom;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::conn_write::ConnWriteSideCustom;
use crate::common::sender::CommonSender;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::stream::HttpStreamDataSpecific;
use crate::common::stream::InMessageStage;
use crate::common::stream_map::HttpStreamRef;
use crate::data_or_headers::DataOrHeaders;
use crate::death::channel::DeathAwareSender;
use crate::death::channel::ErrorAwareDrop;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::headers_place::ClientHeadersPlace;
use crate::net::connect::ToClientStream;
use crate::net::socket::SocketStream;
use crate::req_resp::RequestOrResponse;
use crate::solicit::stream_id::StreamId;
use crate::ClientConf;
use crate::ClientTlsOption;
use crate::ErrorCode;
use bytes::Bytes;
use futures::channel::oneshot;

use crate::client::resp::ClientResponse;
use crate::death::oneshot_no_content_drop::death_aware_oneshot_no_content_drop;
use crate::death::oneshot_no_content_drop::DeathAwareOneshotNoContentDropSender;
use futures::future;
use futures::TryFutureExt;
use tokio::runtime::Handle;

pub struct ClientStreamData {}

impl HttpStreamDataSpecific for ClientStreamData {}

pub(crate) type ClientStream = HttpStreamCommon<ClientTypes>;

impl HttpStreamData for ClientStream {
    type Types = ClientTypes;
}

pub struct ClientConnData {
    _callbacks: Box<dyn ClientConnCallbacks>,
}

impl SideSpecific for ClientConnData {}

pub struct ClientConn {
    internals: ClientInternals,
    write_tx: DeathAwareSender<ClientToWriteMessage, ConnDiedType>,
    pub(crate) conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,
}

unsafe impl Sync for ClientConn {}

pub(crate) struct StartRequestMessage {
    pub headers: Headers,
    pub body: Option<Bytes>,
    pub trailers: Option<Headers>,
    pub end_stream: EndStream,
    pub stream_handler: Box<dyn ClientHandler>,
}

impl fmt::Debug for StartRequestMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let StartRequestMessage {
            headers,
            body,
            trailers,
            end_stream,
            stream_handler,
        } = self;
        let _ = stream_handler;
        f.debug_struct("StartRequestMessage")
            .field("headers", headers)
            .field("body", body)
            .field("trailers", trailers)
            .field("end_stream", end_stream)
            .field("stream_handler", &"...")
            .finish()
    }
}

#[derive(Debug)]
pub struct ClientStartRequestMessage {
    start: StartRequestMessage,
    write_tx: DeathAwareSender<ClientToWriteMessage, ConnDiedType>,
}

#[derive(Debug)]
pub(crate) enum ClientToWriteMessage {
    Start(ClientStartRequestMessage),
    WaitForHandshake(oneshot::Sender<crate::Result<()>>),
    Common(CommonToWriteMessage),
}

impl ErrorAwareDrop for ClientToWriteMessage {
    fn drop_with_error(self, error: crate::Error) {
        match self {
            ClientToWriteMessage::Start(start) => start.start.stream_handler.error(error),
            ClientToWriteMessage::WaitForHandshake(wait) => drop(wait.send(Err(error))),
            ClientToWriteMessage::Common(common) => common.drop_with_error(error),
        }
    }
}

impl From<CommonToWriteMessage> for ClientToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ClientToWriteMessage::Common(m)
    }
}

impl<I> ConnWriteSideCustom for Conn<ClientTypes, I>
where
    I: AsyncSocket,
{
    type Types = ClientTypes;

    fn process_message(&mut self, message: ClientToWriteMessage) -> crate::Result<()> {
        match message {
            ClientToWriteMessage::Start(start) => self.process_start(start),
            ClientToWriteMessage::Common(common) => self.process_common_message(common),
            ClientToWriteMessage::WaitForHandshake(tx) => {
                // ignore error
                drop(tx.send(Ok(())));
                Ok(())
            }
        }
    }
}

impl<I> Conn<ClientTypes, I>
where
    I: AsyncSocket,
{
    fn process_start(&mut self, start: ClientStartRequestMessage) -> crate::Result<()> {
        let ClientStartRequestMessage {
            start:
                StartRequestMessage {
                    headers,
                    body,
                    trailers,
                    end_stream,
                    stream_handler,
                },
            write_tx,
        } = start;

        let stream_id = self.next_local_stream_id();

        {
            let (_, out_window) = self.new_stream_data(
                stream_id,
                None,
                InMessageStage::Initial,
                ClientStreamData {},
            );

            let in_window_size = self
                .streams
                .get_mut(stream_id)
                .unwrap()
                .stream()
                .in_window_size
                .size() as u32;

            let req = match end_stream {
                EndStream::Yes => CommonSender::<ClientTypes>::new_done(stream_id),
                EndStream::No => {
                    CommonSender::<ClientTypes>::new(stream_id, write_tx, out_window, true)
                }
            };

            let mut handler = None;
            let resp = ClientResponse {
                stream_handler: &mut handler,
                in_window_size,
                stream_id,
                to_write_tx: &self.to_write_tx,
                conn_died: &self.conn_died_error_holder,
            };

            match stream_handler.request_created(Box::pin(req), resp) {
                Err(e) => {
                    warn!("client cancelled request: {:?}", e);
                    // Should be fine to cancel before start, but TODO: check
                    self.streams
                        .get_mut(stream_id)
                        .unwrap()
                        .close_outgoing(ErrorCode::InternalError);
                }
                Ok(()) => {
                    let mut stream = self.streams.get_mut(stream_id).unwrap();
                    stream.stream().peer_tx = handler;

                    stream.push_back(DataOrHeaders::Headers(headers));
                    if let Some(body) = body {
                        stream.push_back(DataOrHeaders::Data(body));
                    }
                    if let Some(trailers) = trailers {
                        stream.push_back(DataOrHeaders::Headers(trailers));
                    }
                    if end_stream == EndStream::Yes {
                        stream.close_outgoing(ErrorCode::NoError);
                    }
                }
            };
        }

        // Also opens latch if necessary
        self.buffer_outg_conn()?;
        Ok(())
    }
}

pub trait ClientConnCallbacks: Send + 'static {
    // called at most once
    fn goaway(&self, stream_id: StreamId, raw_error_code: u32);
}

impl ClientConn {
    fn spawn_connected<I, C>(
        lh: Handle,
        connect: impl Future<
                Output = crate::Result<(
                    AnySocketAddr,
                    impl Future<Output = crate::Result<I>> + Send + 'static,
                )>,
            > + Send
            + 'static,
        internals: ClientInternals,
        conf: ClientConf,
        callbacks: C,
    ) -> Self
    where
        I: AsyncSocket,
        C: ClientConnCallbacks,
    {
        let (future, write_tx, conn_died_error_holder) = Conn::<ClientTypes, _>::new(
            lh.clone(),
            ClientConnData {
                _callbacks: Box::new(callbacks),
            },
            conf.common,
            connect,
        );

        lh.spawn(future);

        ClientConn {
            write_tx,
            internals,
            conn_died_error_holder,
        }
    }

    pub fn spawn<H>(
        lh: Handle,
        addr: Pin<Box<dyn ToClientStream + Send>>,
        tls: ClientTlsOption,
        conf: ClientConf,
        callbacks: H,
    ) -> Self
    where
        H: ClientConnCallbacks,
    {
        match tls {
            ClientTlsOption::Plain => {
                ClientConn::spawn_plain(lh.clone(), addr, HttpScheme::Http, conf, callbacks)
            }
            ClientTlsOption::Tls(domain, connector) => ClientConn::spawn_tls(
                lh.clone(),
                &domain,
                connector,
                addr,
                HttpScheme::Https,
                conf,
                callbacks,
            ),
        }
    }

    pub fn spawn_plain<C>(
        lh: Handle,
        addr: Pin<Box<dyn ToClientStream>>,
        http_scheme: HttpScheme,
        conf: ClientConf,
        callbacks: C,
    ) -> Self
    where
        C: ClientConnCallbacks,
    {
        let no_delay = conf.no_delay.unwrap_or(true);

        let lh_copy = lh.clone();
        let connect_timeout = conf.connect_timeout;
        let connect = async move {
            let connect = addr.connect_with_timeout(&lh_copy, connect_timeout);

            let (peer_addr, socket) = connect.await?;

            info!("connected to {}", peer_addr);

            if socket.is_tcp() {
                socket.set_tcp_nodelay(no_delay)?;
            }

            Ok((peer_addr, future::ok(socket)))
        };

        let internals = ClientInternals { http_scheme };

        ClientConn::spawn_connected(lh, connect, internals, conf, callbacks)
    }

    pub fn spawn_tls<H>(
        lh: Handle,
        domain: &str,
        connector: Arc<TlsConnectorBox>,
        addr: Pin<Box<dyn ToClientStream>>,
        http_scheme: HttpScheme,
        conf: ClientConf,
        callbacks: H,
    ) -> Self
    where
        H: ClientConnCallbacks,
    {
        let domain = domain.to_owned();
        let no_delay = conf.no_delay.unwrap_or(true);
        let lh_copy = lh.clone();
        let connect_timeout = conf.connect_timeout;
        let connect = async move {
            let (peer_addr, socket) = addr.connect_with_timeout(&lh_copy, connect_timeout).await?;
            info!("connected to {}", peer_addr);

            if socket.is_tcp() {
                socket
                    .set_tcp_nodelay(no_delay)
                    .map_err(crate::Error::from)?;
            }

            Ok((peer_addr, async move {
                let tls_stream = connector
                    .connect(&domain, socket)
                    .map_err(crate::Error::TlsError)
                    .await?;
                debug!("TLS handshake done");
                Ok(tls_stream)
            }))
        };

        let internals = ClientInternals { http_scheme };

        ClientConn::spawn_connected(lh, connect, internals, conf, callbacks)
    }

    pub(crate) fn start_request_with_resp_sender(
        &self,
        start: StartRequestMessage,
    ) -> Result<(), (StartRequestMessage, crate::Error)> {
        let client_start = ClientStartRequestMessage {
            start,
            write_tx: self.write_tx.clone(),
        };

        self.write_tx
            .unbounded_send_recover(ClientToWriteMessage::Start(client_start))
            .map_err(|(sent_message, e)| match sent_message {
                ClientToWriteMessage::Start(start) => (start.start, e),
                _ => unreachable!(),
            })
    }

    pub(crate) fn dump_state_with_resp_sender(
        &self,
        tx: DeathAwareOneshotNoContentDropSender<ConnStateSnapshot, ConnDiedType>,
    ) {
        let message = ClientToWriteMessage::Common(CommonToWriteMessage::DumpState(tx));
        // ignore error
        drop(self.write_tx.unbounded_send(message));
    }

    /// For tests
    #[doc(hidden)]
    pub fn _dump_state(&self) -> TryFutureBox<ConnStateSnapshot> {
        let (tx, rx) = death_aware_oneshot_no_content_drop(self.conn_died_error_holder.clone());

        self.dump_state_with_resp_sender(tx);

        Box::pin(rx)
    }

    pub fn wait_for_connect_with_resp_sender(
        &self,
        tx: oneshot::Sender<crate::Result<()>>,
    ) -> Result<(), oneshot::Sender<crate::Result<()>>> {
        self.write_tx
            .unbounded_send_recover(ClientToWriteMessage::WaitForHandshake(tx))
            .map_err(|(send_message, _)| match send_message {
                ClientToWriteMessage::WaitForHandshake(tx) => tx,
                _ => unreachable!(),
            })
    }
}

impl ClientIntf for ClientConn {
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: EndStream,
        stream_handler: Box<dyn ClientHandler>,
    ) {
        let start = StartRequestMessage {
            headers,
            body,
            trailers,
            end_stream,
            stream_handler,
        };

        if let Err((start, e)) = self.start_request_with_resp_sender(start) {
            start.stream_handler.error(e);
        }
    }

    fn internals(&self) -> &ClientInternals {
        &self.internals
    }
}

impl<I> ConnReadSideCustom for Conn<ClientTypes, I>
where
    I: AsyncSocket,
{
    type Types = ClientTypes;

    fn process_headers(
        &mut self,
        stream_id: StreamId,
        end_stream: EndStream,
        headers: Headers,
    ) -> crate::Result<Option<HttpStreamRef<ClientTypes>>> {
        let existing_stream = self
            .get_stream_for_headers_maybe_send_error(stream_id)?
            .is_some();
        if !existing_stream {
            return Ok(None);
        }

        let in_message_stage = self
            .streams
            .get_mut(stream_id)
            .unwrap()
            .stream()
            .in_message_stage;

        let headers_place = match in_message_stage {
            InMessageStage::Initial => {
                // TODO: handle error
                let status = headers.status();

                let status_1xx = status >= 100 && status <= 199;

                match (status_1xx, end_stream) {
                    (true, EndStream::Yes) => {
                        warn!("1xx headers and end stream: {}", stream_id);
                        self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                        return Ok(None);
                    }
                    (true, EndStream::No) => ClientHeadersPlace::Initial1Xx,
                    (false, EndStream::Yes) => ClientHeadersPlace::InitialEndOfStream,
                    (false, EndStream::No) => ClientHeadersPlace::Initial,
                }
            }
            InMessageStage::AfterInitialHeaders => {
                if end_stream == EndStream::No {
                    warn!("headers without end stream after data: {}", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                    return Ok(None);
                }
                ClientHeadersPlace::Trailing
            }
            InMessageStage::AfterTrailingHeaders => {
                return Err(crate::Error::InternalError(format!(
                    "closed stream must be handled before"
                )));
            }
        };

        if let Err(e) = headers.validate(
            RequestOrResponse::Response,
            headers_place.to_headers_place(),
        ) {
            warn!("invalid headers: {:?}: {:?}", e, headers);
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        if let Some(in_rem_content_length) = headers.content_length() {
            stream.stream().in_rem_content_length = Some(in_rem_content_length);
        }

        stream.stream().in_message_stage = match headers_place {
            ClientHeadersPlace::Initial1Xx => InMessageStage::Initial,
            ClientHeadersPlace::Initial => InMessageStage::AfterInitialHeaders,
            ClientHeadersPlace::InitialEndOfStream => InMessageStage::AfterTrailingHeaders,
            ClientHeadersPlace::Trailing => InMessageStage::AfterTrailingHeaders,
        };

        // Ignore 1xx headers:
        // 8.1.  HTTP Request/Response Exchange
        // ...
        //    An HTTP message (request or response) consists of:
        //    1.  for a response only, zero or more HEADERS frames (each followed
        //        by zero or more CONTINUATION frames) containing the message
        //        headers of informational (1xx) HTTP responses (see [RFC7230],
        //        Section 3.2 and [RFC7231], Section 6.2),
        // TODO: reset stream on error
        match headers_place {
            ClientHeadersPlace::Initial1Xx => {}
            ClientHeadersPlace::Initial => {
                if let Some(ref mut response_handler) = stream.stream().peer_tx {
                    let _ = response_handler.0.headers(headers, EndStream::No);
                }
            }
            ClientHeadersPlace::InitialEndOfStream => {
                if let Some(mut response_handler) = stream.stream().peer_tx.take() {
                    let _ = response_handler.0.headers(headers, EndStream::Yes);
                }
            }
            ClientHeadersPlace::Trailing => {
                if let Some(response_handler) = stream.stream().peer_tx.take() {
                    let _ = response_handler.0.trailers(headers);
                }
            }
        }

        Ok(Some(stream))
    }
}
