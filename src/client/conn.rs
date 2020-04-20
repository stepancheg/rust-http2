//! Single client connection

use std::io;
use std::result::Result as std_Result;
use std::sync::Arc;

use crate::error;
use crate::error::Error;
use crate::result;
use crate::AnySocketAddr;

use crate::solicit::end_stream::EndStream;
use crate::solicit::frame::HttpSetting;
use crate::solicit::frame::SettingsFrame;
use crate::solicit::header::*;
use crate::solicit::DEFAULT_SETTINGS;

use std::future::Future;

use tls_api::TlsConnector;

use tls_api;

use crate::solicit_async::*;

use crate::assert_types::assert_send_future;
use crate::client::increase_in_window::ClientIncreaseInWindow;
use crate::client::req::ClientRequest;
use crate::client::stream_handler::ClientStreamCreatedHandler;
use crate::client::stream_handler::ClientStreamHandlerHolder;
use crate::client::types::ClientTypes;
use crate::client::ClientInterface;
use crate::client_died_error_holder::SomethingDiedErrorHolder;
use crate::common::conn::Conn;
use crate::common::conn::ConnSpecific;
use crate::common::conn::ConnStateSnapshot;
use crate::common::conn_command_channel::conn_command_channel;
use crate::common::conn_command_channel::ConnCommandSender;
use crate::common::conn_read::ConnReadSideCustom;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::conn_write::ConnWriteSideCustom;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::sender::CommonSender;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::stream::HttpStreamDataSpecific;
use crate::common::stream::InMessageStage;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::common::stream_map::HttpStreamRef;
use crate::data_or_headers::DataOrHeaders;
use crate::headers_place::HeadersPlace;
use crate::req_resp::RequestOrResponse;
use crate::socket::StreamItem;
use crate::socket::ToClientStream;
use crate::solicit::stream_id::StreamId;
use crate::ClientConf;
use crate::ClientTlsOption;
use crate::ErrorCode;
use bytes::Bytes;
use futures::channel::oneshot;
use futures::FutureExt;
use futures::TryFutureExt;
use std::pin::Pin;

use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
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

impl ConnSpecific for ClientConnData {}

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
    I: AsyncWrite + AsyncRead + Send + 'static,
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
    I: AsyncWrite + AsyncRead + Send + 'static,
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

            let increase_in_window = ClientIncreaseInWindow(IncreaseInWindow {
                stream_id,
                in_window_size,
                to_write_tx: self.to_write_tx.clone(),
            });

            let req = ClientRequest {
                common: if end_stream {
                    CommonSender::new_done(stream_id)
                } else {
                    CommonSender::new(stream_id, write_tx, out_window, true)
                },
                drop_callback: None,
            };

            match stream_handler.request_created(req, increase_in_window) {
                Err(e) => {
                    warn!("client cancelled request: {:?}", e);
                    // Should be fine to cancel before start, but TODO: check
                    self.streams
                        .get_mut(stream_id)
                        .unwrap()
                        .close_outgoing(ErrorCode::InternalError);
                }
                Ok(handler) => {
                    let mut stream = self.streams.get_mut(stream_id).unwrap();
                    stream.stream().peer_tx = Some(ClientStreamHandlerHolder(handler));

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
        I: AsyncWrite + AsyncRead + Unpin + Send + 'static,
        C: ClientConnCallbacks,
    {
        let conn_died_error_holder = SomethingDiedErrorHolder::new();

        let (to_write_tx, to_write_rx) = conn_command_channel(conn_died_error_holder.clone());

        let c = ClientConn {
            write_tx: to_write_tx.clone(),
        };

        let settings_frame = SettingsFrame::from_settings(vec![HttpSetting::EnablePush(false)]);
        let mut settings = DEFAULT_SETTINGS;
        settings.apply_from_frame(&settings_frame);

        let lh_copy = lh.clone();

        let conn_died_error_holder_copy = conn_died_error_holder.clone();

        let future = connect.and_then(move |mut conn| async move {
            client_handshake(&mut conn, settings_frame).await?;

            debug!("handshake done");

            let conn_data = Conn::<ClientTypes, _>::new(
                lh_copy,
                ClientConnData {
                    _callbacks: Box::new(callbacks),
                },
                conf.common,
                settings,
                to_write_tx.clone(),
                to_write_rx,
                conn,
                peer_addr,
                conn_died_error_holder,
            );
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
        let map_callback = move |socket: Pin<Box<dyn StreamItem + Send>>| {
            info!("connected to {}", addr);

            if socket.is_tcp() {
                socket
                    .set_nodelay(no_delay)
                    .expect("failed to set TCP_NODELAY");
            }

            socket
        };

        let connect: Pin<
            Box<dyn Future<Output = result::Result<Pin<Box<dyn StreamItem + Send>>>> + Send>,
        > = if let Some(timeout) = conf.connection_timeout {
            Box::pin(time::timeout(timeout, connect).map(|r| match r {
                Ok(r) => r,
                Err(_) => Err(error::Error::ConnectionTimeout),
            }))
        } else {
            Box::pin(connect)
        };

        let connect = Box::pin(
            connect.map_ok(move |socket: Pin<Box<dyn StreamItem + Send>>| map_callback(socket)),
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

        let connect = addr
            .connect(&lh)
            .map(move |c| {
                info!("connected to {}", addr);
                c
            })
            .map_err(|e| error::Error::from(e));

        let connect = assert_send_future(connect);

        let tls_conn = connect
            .and_then(move |conn| async move { Ok(connector.connect(&domain, conn).await?) });

        let tls_conn = assert_send_future(tls_conn);

        let tls_conn = tls_conn.map_err(Error::from);

        let tls_conn = assert_send_future(tls_conn);

        ClientConn::spawn_connected(lh, Box::pin(tls_conn), addr_struct, conf, callbacks)
    }

    pub(crate) fn start_request_with_resp_sender(
        &self,
        start: StartRequestMessage,
    ) -> Result<(), StartRequestMessage> {
        let client_start = ClientStartRequestMessage {
            start,
            write_tx: self.write_tx.clone(),
        };

        self.write_tx
            .unbounded_send_recover(ClientToWriteMessage::Start(client_start))
            .map_err(|send_error| match send_error {
                ClientToWriteMessage::Start(start) => start.start,
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
            .map_err(|send_error| match send_error {
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

        if let Err(_) = self.start_request_with_resp_sender(start) {
            return Err(error::Error::ClientDied(None));
        }

        Ok(())
    }
}

impl<I> ConnReadSideCustom for Conn<ClientTypes, I>
where
    I: AsyncWrite + AsyncRead + Send + 'static,
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
