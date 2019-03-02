use std::panic;
use std::sync::Arc;

use error;
use result;

use solicit::end_stream::EndStream;
use solicit::frame::settings::*;
use solicit::header::*;
use solicit::DEFAULT_SETTINGS;

use futures::future;
use futures::future::Future;
use futures::sync::oneshot;

use common::types::Types;
use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use tokio_tls_api;

use tls_api::TlsAcceptor;
use tls_api_stub;

use solicit_async::*;

use socket::StreamItem;

use common::init_where::InitWhere;

use client_died_error_holder::SomethingDiedErrorHolder;
use common::conn::Conn;
use common::conn::ConnSpecific;
use common::conn::ConnStateSnapshot;
use common::conn_command_channel::conn_command_channel;
use common::conn_command_channel::ConnCommandSender;
use common::conn_read::ConnReadSideCustom;
use common::conn_write::CommonToWriteMessage;
use common::conn_write::ConnWriteSideCustom;
use common::sender::CommonSender;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use common::stream::HttpStreamDataSpecific;
use common::stream::InMessageStage;
use common::stream_handler::StreamHandlerInternal;
use common::stream_map::HttpStreamRef;
use headers_place::HeadersPlace;
use misc::any_to_string;
use req_resp::RequestOrResponse;
use server::handler::ServerHandler;
use server::handler::ServerHandlerContext;
use server::req::ServerRequest;
use server::types::ServerTypes;
use solicit::stream_id::StreamId;
use ErrorCode;
use ServerConf;
use ServerResponse;
use ServerTlsOption;

pub struct ServerStreamData {}

impl HttpStreamDataSpecific for ServerStreamData {}

pub(crate) type ServerStream = HttpStreamCommon<ServerTypes>;

impl ServerStream {
    fn trailers_recvd(&mut self, headers: Headers) {
        if let Some(ref mut sender) = self.peer_tx {
            // TODO: reset on error
            sender.trailers(headers).ok();
        }
    }
}

impl HttpStreamData for ServerStream {
    type Types = ServerTypes;
}

pub(crate) struct ServerConnData {
    factory: Arc<ServerHandler>,
}

impl ConnSpecific for ServerConnData {}

#[allow(dead_code)] // https://github.com/rust-lang/rust/issues/42303
type ServerInner<I> = Conn<ServerTypes, I>;

impl<I> ServerInner<I>
where
    I: AsyncWrite + AsyncRead + Send + 'static,
{
    fn new_stream_from_client(
        &mut self,
        stream_id: StreamId,
        headers: Headers,
        end_stream: EndStream,
    ) -> result::Result<HttpStreamRef<ServerTypes>> {
        if ServerTypes::init_where(stream_id) == InitWhere::Locally {
            return Err(error::Error::InitiatedStreamWithServerIdFromClient(
                stream_id,
            ));
        }

        if stream_id <= self.last_peer_stream_id {
            return Err(error::Error::StreamIdLeExistingStream(
                stream_id,
                self.last_peer_stream_id,
            ));
        }

        self.last_peer_stream_id = stream_id;

        debug!("new stream: {}", stream_id);

        let (_, out_window) = self.new_stream_data(
            stream_id,
            headers.content_length(),
            InMessageStage::AfterInitialHeaders,
            ServerStreamData {},
        );

        let in_window_size = self
            .streams
            .get_mut(stream_id)
            .unwrap()
            .stream()
            .in_window_size
            .size() as u32;

        let factory = self.specific.factory.clone();

        let sender = ServerResponse {
            common: CommonSender::new(stream_id, self.to_write_tx.clone(), out_window, false),
            drop_callback: None,
        };

        let context = ServerHandlerContext {
            loop_handle: self.loop_handle.remote().clone(),
        };

        let mut stream_handler = None;
        let invoke_result = {
            let req = ServerRequest {
                headers,
                end_stream: end_stream == EndStream::Yes,
                stream_id,
                in_window_size,
                stream_handler: &mut stream_handler,
                to_write_tx: &self.to_write_tx,
            };

            panic::catch_unwind(panic::AssertUnwindSafe(|| {
                factory.start_request(context, req, sender)
            }))
        };

        let mut stream = self.streams.get_mut(stream_id).expect("get stream");

        match invoke_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warn!("handler returned error: {:?}", e);
                // Not closing stream because sender object
                // is now responsible for sending `RST_STREAM` on error.
            }
            Err(e) => {
                let e = any_to_string(e);
                warn!("handler panicked: {}", e);
            }
        }

        stream.stream().peer_tx = stream_handler;

        Ok(stream)
    }
}

pub enum ServerToWriteMessage {
    Common(CommonToWriteMessage),
}

impl From<CommonToWriteMessage> for ServerToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ServerToWriteMessage::Common(m)
    }
}

impl<I> ConnWriteSideCustom for Conn<ServerTypes, I>
where
    I: AsyncWrite + AsyncRead + Send + 'static,
{
    type Types = ServerTypes;

    fn process_message(&mut self, message: ServerToWriteMessage) -> result::Result<()> {
        match message {
            ServerToWriteMessage::Common(common) => self.process_common_message(common),
        }
    }
}

impl<I> ConnReadSideCustom for Conn<ServerTypes, I>
where
    I: AsyncWrite + AsyncRead + Send + 'static,
{
    type Types = ServerTypes;

    fn process_headers(
        &mut self,
        stream_id: StreamId,
        end_stream: EndStream,
        headers: Headers,
    ) -> result::Result<Option<HttpStreamRef<ServerTypes>>> {
        let existing_stream = self
            .get_stream_for_headers_maybe_send_error(stream_id)?
            .is_some();

        let headers_place = match existing_stream {
            true => HeadersPlace::Trailing,
            false => HeadersPlace::Initial,
        };

        if let Err(e) = headers.validate(RequestOrResponse::Request, headers_place) {
            warn!("invalid headers: {:?} {:?}", e, headers);
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        if !existing_stream {
            return self
                .new_stream_from_client(stream_id, headers, end_stream)
                .map(Some);
        }

        if end_stream == EndStream::No {
            warn!("more headers without end stream flag");
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        stream.stream().trailers_recvd(headers);
        Ok(Some(stream))
    }
}

pub struct ServerConn {
    write_tx: ConnCommandSender<ServerTypes>,
}

impl ServerConn {
    fn connected<F, I>(
        lh: &reactor::Handle,
        socket: HttpFutureSend<I>,
        conf: ServerConf,
        service: Arc<F>,
    ) -> (ServerConn, HttpFuture<()>)
    where
        F: ServerHandler,
        I: AsyncRead + AsyncWrite + Send + 'static,
    {
        let lh = lh.clone();

        let conn_died_error_holder = SomethingDiedErrorHolder::new();

        let (write_tx, write_rx) = conn_command_channel(conn_died_error_holder.clone());

        let settings_frame = SettingsFrame::from_settings(vec![HttpSetting::EnablePush(false)]);
        let mut settings = DEFAULT_SETTINGS;
        settings.apply_from_frame(&settings_frame);

        let handshake = socket.and_then(|conn| server_handshake(conn, settings_frame));

        let write_tx_copy = write_tx.clone();

        let run = handshake.and_then(move |conn| {
            let (read, write) = conn.split();

            let conn_data = Conn::<ServerTypes, I>::new(
                lh,
                ServerConnData { factory: service },
                conf.common,
                settings,
                write_tx_copy,
                write_rx,
                read,
                write,
                conn_died_error_holder,
            );

            conn_data.run()
        });

        let future = Box::new(run.then(|x| {
            info!("connection end: {:?}", x);
            x
        }));

        (ServerConn { write_tx }, future)
    }

    pub fn new<S, A>(
        lh: &reactor::Handle,
        socket: Box<StreamItem>,
        tls: ServerTlsOption<A>,
        conf: ServerConf,
        service: Arc<S>,
    ) -> (ServerConn, HttpFuture<()>)
    where
        S: ServerHandler,
        A: TlsAcceptor,
    {
        match tls {
            ServerTlsOption::Plain => {
                let socket = Box::new(future::finished(socket));
                ServerConn::connected(lh, socket, conf, service)
            }
            ServerTlsOption::Tls(acceptor) => {
                let socket = Box::new(
                    tokio_tls_api::accept_async(&*acceptor, socket).map_err(error::Error::from),
                );
                ServerConn::connected(lh, socket, conf, service)
            }
        }
    }

    pub fn new_plain_single_thread<S>(
        lh: &reactor::Handle,
        socket: TcpStream,
        conf: ServerConf,
        service: Arc<S>,
    ) -> (ServerConn, HttpFuture<()>)
    where
        S: ServerHandler,
    {
        let no_tls: ServerTlsOption<tls_api_stub::TlsAcceptor> = ServerTlsOption::Plain;
        ServerConn::new(lh, Box::new(socket), no_tls, conf, service)
    }

    pub fn new_plain_single_thread_fn<F>(
        lh: &reactor::Handle,
        socket: TcpStream,
        conf: ServerConf,
        f: F,
    ) -> (ServerConn, HttpFuture<()>)
    where
        F: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> result::Result<()>
            + Send
            + Sync
            + 'static,
    {
        struct HttpServiceFn<F>(F);

        impl<F> ServerHandler for HttpServiceFn<F>
        where
            F: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> result::Result<()>
                + Send
                + Sync
                + 'static,
        {
            fn start_request(
                &self,
                context: ServerHandlerContext,
                req: ServerRequest,
                resp: ServerResponse,
            ) -> result::Result<()> {
                (self.0)(context, req, resp)
            }
        }

        ServerConn::new_plain_single_thread(lh, socket, conf, Arc::new(HttpServiceFn(f)))
    }

    /// For tests
    pub fn dump_state(&self) -> HttpFutureSend<ConnStateSnapshot> {
        let (tx, rx) = oneshot::channel();

        if let Err(_) = self.write_tx.unbounded_send(ServerToWriteMessage::Common(
            CommonToWriteMessage::DumpState(tx),
        )) {
            return Box::new(future::err(error::Error::FailedToSendReqToDumpState));
        }

        let rx = rx.map_err(|_| error::Error::OneshotCancelled);

        Box::new(rx)
    }
}
