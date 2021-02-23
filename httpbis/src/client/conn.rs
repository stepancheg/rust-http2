//! Single client connection

use std::io;
use std::result::Result as std_Result;
use std::sync::Arc;

use crate::error;
use crate::error::Error;
use crate::result;
use crate::AnySocketAddr;

use crate::solicit::end_stream::EndStream;
use crate::solicit::header::*;

use std::future::Future;

use tls_api::TlsConnector;

use tls_api;

use crate::solicit_async::*;

use crate::assert_types::assert_send_future;
use crate::client::req::ClientRequest;

use crate::client::stream_handler::ClientStreamCreatedHandler;
use crate::client::types::ClientTypes;
use crate::client::ClientInterface;
use crate::client_died_error_holder::SomethingDiedErrorHolder;
use crate::common::conn::Conn;
use crate::common::conn::ConnStateSnapshot;
use crate::common::conn::SideSpecific;
use crate::common::conn_command_channel::conn_command_channel;
use crate::common::conn_command_channel::ConnCommandSender;
use crate::common::conn_read::ConnReadSideCustom;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::conn_write::ConnWriteSideCustom;
use crate::common::sender::CommonSender;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::stream::HttpStreamDataSpecific;
use crate::common::stream::InMessageStage;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::common::stream_map::HttpStreamRef;
use crate::data_or_headers::DataOrHeaders;
use crate::headers_place::HeadersPlace;
use crate::net::connect::ToClientStream;
use crate::net::socket::SocketStream;
use crate::req_resp::RequestOrResponse;
use crate::solicit::stream_id::StreamId;
use crate::ClientConf;
use crate::ClientTlsOption;
use crate::ErrorCode;
use bytes::Bytes;
use futures::channel::oneshot;
use futures::FutureExt;
use futures::TryFutureExt;
use std::pin::Pin;

use crate::client::resp::ClientResponse;
use tokio::runtime::Handle;
use tokio::time;

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
    write_tx: ConnCommandSender<ClientTypes>,
}

unsafe impl Sync for ClientConn {}

pub(crate) struct StartRequestMessage {
    pub headers: Headers,
    pub body: Option<Bytes>,
    pub trailers: Option<Headers>,
    pub end_stream: bool,
    pub stream_handler: Box<dyn ClientStreamCreatedHandler>,
}

pub struct ClientStartRequestMessage {
    start: StartRequestMessage,
    write_tx: ConnCommandSender<ClientTypes>,
}

pub(crate) enum ClientToWriteMessage {
    Start(ClientStartRequestMessage),
    WaitForHandshake(oneshot::Sender<result::Result<()>>),
    Common(CommonToWriteMessage),
}

impl From<CommonToWriteMessage> for ClientToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ClientToWriteMessage::Common(m)
    }
}

impl<I> ConnWriteSideCustom for Conn<ClientTypes, I>
where
    I: SocketStream,
{
    type Types = ClientTypes;

    fn process_message(&mut self, message: ClientToWriteMessage) -> result::Result<()> {
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
    I: SocketStream,
{
    fn process_start(&mut self, start: ClientStartRequestMessage) -> result::Result<()> {
        let ClientStartRequestMessage {
            start:
                StartRequestMessage {
                    headers,
                    body,
                    trailers,
                    end_stream,
                    mut stream_handler,
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

            let req = ClientRequest {
                common: if end_stream {
                    CommonSender::new_done(stream_id)
                } else {
                    CommonSender::new(stream_id, write_tx, out_window, true)
                },
                drop_callback: None,
            };

            let mut handler = None;
            let resp = ClientResponse {
                stream_handler: &mut handler,
                in_window_size,
                stream_id,
                to_write_tx: &self.to_write_tx,
            };

            match stream_handler.request_created(req, resp) {
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
                    if end_stream {
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
        connect: HttpFutureSend<I>,
        peer_addr: AnySocketAddr,
        conf: ClientConf,
        callbacks: C,
    ) -> Self
    where
        I: SocketStream,
        C: ClientConnCallbacks,
    {
        let conn_died_error_holder = SomethingDiedErrorHolder::new();

        let (to_write_tx, to_write_rx) = conn_command_channel(conn_died_error_holder.clone());

        let c = ClientConn {
            write_tx: to_write_tx.clone(),
        };

        let lh_copy = lh.clone();

        let conn_died_error_holder_copy = conn_died_error_holder.clone();

        let future = connect.and_then(move |conn| async move {
            let conn_data = Conn::<ClientTypes, _>::new(
                lh_copy,
                ClientConnData {
                    _callbacks: Box::new(callbacks),
                },
                conf.common,
                to_write_tx.clone(),
                to_write_rx,
                conn,
                peer_addr,
                conn_died_error_holder,
            )
            .await?;
            conn_data.run().await
        });

        let future = conn_died_error_holder_copy.wrap_future(future);

        lh.spawn(future);

        c
    }

    pub fn spawn<H, C>(
        lh: Handle,
        addr: Pin<Box<dyn ToClientStream + Send>>,
        tls: ClientTlsOption<C>,
        conf: ClientConf,
        callbacks: H,
    ) -> Self
    where
        H: ClientConnCallbacks,
        C: TlsConnector + Sync,
    {
        match tls {
            ClientTlsOption::Plain => ClientConn::spawn_plain(lh.clone(), addr, conf, callbacks),
            ClientTlsOption::Tls(domain, connector) => {
                ClientConn::spawn_tls(lh.clone(), &domain, connector, addr, conf, callbacks)
            }
        }
    }

    pub fn spawn_plain<C>(
        lh: Handle,
        addr: Pin<Box<dyn ToClientStream>>,
        conf: ClientConf,
        callbacks: C,
    ) -> Self
    where
        C: ClientConnCallbacks,
    {
        let addr_struct = addr.socket_addr();

        let no_delay = conf.no_delay.unwrap_or(true);
        let connect = TryFutureExt::map_err(addr.connect(&lh), error::Error::from);
        let map_callback = move |socket: Pin<Box<dyn SocketStream>>| {
            info!("connected to {}", addr);

            if socket.is_tcp() {
                socket
                    .set_tcp_nodelay(no_delay)
                    .expect("failed to set TCP_NODELAY");
            }

            socket
        };

        let connect: Pin<
            Box<dyn Future<Output = result::Result<Pin<Box<dyn SocketStream>>>> + Send>,
        > = if let Some(timeout) = conf.connection_timeout {
            Box::pin(time::timeout(timeout, connect).map(|r| match r {
                Ok(r) => r,
                Err(_) => Err(error::Error::ConnectionTimeout),
            }))
        } else {
            Box::pin(connect)
        };

        let connect = Box::pin(
            connect.map_ok(move |socket: Pin<Box<dyn SocketStream>>| map_callback(socket)),
        );

        ClientConn::spawn_connected(lh, connect, addr_struct, conf, callbacks)
    }

    pub fn spawn_tls<H, C>(
        lh: Handle,
        domain: &str,
        connector: Arc<C>,
        addr: Pin<Box<dyn ToClientStream + Send>>,
        conf: ClientConf,
        callbacks: H,
    ) -> Self
    where
        H: ClientConnCallbacks,
        C: TlsConnector + Sync,
    {
        let addr_struct = addr.socket_addr();

        let domain = domain.to_owned();

        let no_delay = conf.no_delay.unwrap_or(true);
        let connect = addr
            .connect(&lh)
            .map_ok(move |socket| {
                info!("connected to {}", addr);

                if socket.is_tcp() {
                    socket
                        .set_tcp_nodelay(no_delay)
                        .expect("failed to set TCP_NODELAY");
                }
                socket
            })
            .map_err(|e| error::Error::from(e));

        let connect = assert_send_future(connect);

        let tls_conn = connect.and_then(move |conn| async move {
            Ok(connector.connect_with_socket(&domain, conn).await?)
        });

        let tls_conn = assert_send_future(tls_conn);

        let tls_conn = tls_conn.map_err(Error::from);

        let tls_conn = assert_send_future(tls_conn);

        ClientConn::spawn_connected(lh, Box::pin(tls_conn), addr_struct, conf, callbacks)
    }

    pub(crate) fn start_request_with_resp_sender(
        &self,
        start: StartRequestMessage,
    ) -> Result<(), (StartRequestMessage, error::Error)> {
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

    pub fn dump_state_with_resp_sender(&self, tx: oneshot::Sender<ConnStateSnapshot>) {
        let message = ClientToWriteMessage::Common(CommonToWriteMessage::DumpState(tx));
        // ignore error
        drop(self.write_tx.unbounded_send(message));
    }

    /// For tests
    #[doc(hidden)]
    pub fn _dump_state(&self) -> HttpFutureSend<ConnStateSnapshot> {
        let (tx, rx) = oneshot::channel();

        self.dump_state_with_resp_sender(tx);

        let rx =
            rx.map_err(|_| Error::from(io::Error::new(io::ErrorKind::Other, "oneshot canceled")));

        Box::pin(rx)
    }

    pub fn wait_for_connect_with_resp_sender(
        &self,
        tx: oneshot::Sender<result::Result<()>>,
    ) -> std_Result<(), oneshot::Sender<result::Result<()>>> {
        self.write_tx
            .unbounded_send_recover(ClientToWriteMessage::WaitForHandshake(tx))
            .map_err(|(send_message, _)| match send_message {
                ClientToWriteMessage::WaitForHandshake(tx) => tx,
                _ => unreachable!(),
            })
    }
}

impl ClientInterface for ClientConn {
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
        stream_handler: Box<dyn ClientStreamCreatedHandler>,
    ) -> result::Result<()> {
        let start = StartRequestMessage {
            headers,
            body,
            trailers,
            end_stream,
            stream_handler,
        };

        if let Err((_, e)) = self.start_request_with_resp_sender(start) {
            return Err(error::Error::ClientDied(Some(Arc::new(e))));
        }

        Ok(())
    }
}

impl<I> ConnReadSideCustom for Conn<ClientTypes, I>
where
    I: SocketStream,
{
    type Types = ClientTypes;

    fn process_headers(
        &mut self,
        stream_id: StreamId,
        end_stream: EndStream,
        headers: Headers,
    ) -> result::Result<Option<HttpStreamRef<ClientTypes>>> {
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
            InMessageStage::Initial => HeadersPlace::Initial,
            InMessageStage::AfterInitialHeaders => HeadersPlace::Trailing,
            InMessageStage::AfterTrailingHeaders => {
                return Err(error::Error::InternalError(format!(
                    "closed stream must be handled before"
                )));
            }
        };

        if let Err(e) = headers.validate(RequestOrResponse::Response, headers_place) {
            warn!("invalid headers: {:?}: {:?}", e, headers);
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let status_1xx = match headers_place {
            HeadersPlace::Initial => {
                let status = headers.status();

                let status_1xx = status >= 100 && status <= 199;
                if status_1xx && end_stream == EndStream::Yes {
                    warn!("1xx headers and end stream: {}", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                    return Ok(None);
                }
                status_1xx
            }
            HeadersPlace::Trailing => {
                if end_stream == EndStream::No {
                    warn!("headers without end stream after data: {}", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                    return Ok(None);
                }
                false
            }
        };

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        if let Some(in_rem_content_length) = headers.content_length() {
            stream.stream().in_rem_content_length = Some(in_rem_content_length);
        }

        stream.stream().in_message_stage = match (headers_place, status_1xx) {
            (HeadersPlace::Initial, false) => InMessageStage::AfterInitialHeaders,
            (HeadersPlace::Initial, true) => InMessageStage::Initial,
            (HeadersPlace::Trailing, _) => InMessageStage::AfterTrailingHeaders,
        };

        // Ignore 1xx headers
        if !status_1xx {
            if let Some(ref mut response_handler) = stream.stream().peer_tx {
                // TODO: reset stream on error
                drop(match headers_place {
                    HeadersPlace::Initial => response_handler
                        .0
                        .headers(headers, end_stream == EndStream::Yes),
                    HeadersPlace::Trailing => {
                        assert_eq!(EndStream::Yes, end_stream);
                        response_handler.trailers(headers)
                    }
                });
            } else {
                // TODO: reset stream
            }
        }

        Ok(Some(stream))
    }
}
