use std::io;
use std::sync::Arc;
use std::panic;

use error;
use result;

use exec::CpuPoolOption;

use solicit::StreamId;
use solicit::header::*;
use solicit::connection::EndStream;
use solicit::frame::settings::*;
use solicit::DEFAULT_SETTINGS;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream;
use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;
use futures::sync::oneshot;

use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_tls_api;

use tls_api::TlsAcceptor;
use tls_api_stub;

use solicit_async::*;
use service::Service;
use data_or_trailers::*;
use common::*;

use server_tls::*;
use server_conf::*;
use socket::StreamItem;

use misc::any_to_string;
use rc_mut::*;
use req_resp::RequestOrResponse;
use headers_place::HeadersPlace;
use ErrorCode;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use result_or_eof::ResultOrEof;
use codec::http_frame_read::HttpFrameJoinContinuationRead;


struct ServerTypes;

impl Types for ServerTypes {
    type HttpStreamData = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type ConnDataSpecific = ServerConnData;
    type ToWriteMessage = ServerToWriteMessage;

    fn first_id() -> StreamId {
        2
    }

    fn out_request_or_response() -> RequestOrResponse {
        RequestOrResponse::Response
    }
}


pub struct ServerStreamData {
}

impl HttpStreamDataSpecific for ServerStreamData {
}

type ServerStream = HttpStreamCommon<ServerTypes>;

impl ServerStream {
    fn set_headers(&mut self, headers: Headers, last: bool) {
        if let Some(ref mut sender) = self.peer_tx {
            let part = DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(headers),
                last,
            };
            // TODO: reset on error
            sender.send(ResultOrEof::Item(part)).ok();
        }
    }
}

impl HttpStreamData for ServerStream {
    type Types = ServerTypes;
}

struct ServerConnData {
    factory: Arc<Service>,
}

impl ConnDataSpecific for ServerConnData {
}

#[allow(dead_code)] // https://github.com/rust-lang/rust/issues/42303
type ServerInner = ConnData<ServerTypes>;

impl ServerInner {
    fn new_stream_from_client(&mut self, _self_rc: RcMut<Self>, stream_id: StreamId, headers: Headers)
        -> result::Result<HttpStreamRef<ServerTypes>>
    {
        if ServerTypes::is_init_locally(stream_id) {
            return Err(error::Error::Other("initiated stream with server id from client"));
        }

        if stream_id <= self.last_peer_stream_id {
            return Err(error::Error::Other("stream id is le than already existing stream id"));
        }

        self.last_peer_stream_id = stream_id;

        debug!("new stream: {}", stream_id);

        let (_, req_stream, out_window) = self.new_stream_data(
            stream_id,
            headers.content_length(),
            InMessageStage::AfterInitialHeaders,
            ServerStreamData {});

        let req_stream = HttpStreamAfterHeaders::from_parts(req_stream);

        let factory = self.specific.factory.clone();

        let to_write_tx = self.to_write_tx.clone();

        self.exec.execute(Box::new(future::lazy(move || {
            let response = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                // TODO: do start request in executor
                factory.start_request(headers, req_stream)
            }));

            let response = response.unwrap_or_else(|e| {
                let e = any_to_string(e);
                warn!("handler panicked: {}", e);

                let headers = Headers::internal_error_500();
                Response::from_stream(stream::iter_ok(vec![
                    DataOrHeadersWithFlag::intermediate_headers(headers),
                    DataOrHeadersWithFlag::last_data(Bytes::from(format!("handler panicked: {}", e))),
                ]))
            });

            let response = response.into_part_stream();
            let response = response.catch_unwind();

            PumpStreamToWriteLoop::<ServerTypes> {
                to_write_tx: to_write_tx,
                stream_id: stream_id,
                out_window: out_window,
                stream: response,
            }
        })));

        Ok(self.streams.get_mut(stream_id).expect("get stream"))
    }
}

impl ConnInner for ServerInner {
    type Types = ServerTypes;

    fn process_headers(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<ServerTypes>>>
    {
        let existing_stream = self.get_stream_for_headers_maybe_send_error(stream_id)?.is_some();

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
            return self.new_stream_from_client(self_rc, stream_id, headers).map(Some);
        }

        if end_stream == EndStream::No {
            warn!("more headers without end stream flag");
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        stream.stream().set_headers(headers, end_stream == EndStream::Yes);
        Ok(Some(stream))
    }
}

type ServerReadLoop<I> = ReadLoopData<I, ServerTypes>;
type ServerWriteLoop<I> = WriteLoopData<I, ServerTypes>;
type ServerCommandLoop = CommandLoopData<ServerTypes>;


enum ServerToWriteMessage {
    Common(CommonToWriteMessage),
}

impl From<CommonToWriteMessage> for ServerToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ServerToWriteMessage::Common(m)
    }
}

enum ServerCommandMessage {
    DumpState(oneshot::Sender<ConnectionStateSnapshot>),
}


impl<I : AsyncWrite + Send> ServerWriteLoop<I> {
    fn _loop_handle(&self) -> reactor::Handle {
        self.inner.with(move |inner: &mut ServerInner| inner.loop_handle.clone())
    }

    fn process_message(self, message: ServerToWriteMessage) -> HttpFuture<Self> {
        match message {
            ServerToWriteMessage::Common(common) => {
                self.process_common(common)
            },
        }
    }

    fn run(self, requests: HttpFutureStream<ServerToWriteMessage>) -> HttpFuture<()> {
        let requests = requests.map_err(error::Error::from);
        Box::new(requests
            .fold(self, move |wl, message: ServerToWriteMessage| {
                wl.process_message(message)
            })
            .map(|_| ()))
    }
}

impl ServerCommandLoop {
    fn process_dump_state(self, sender: oneshot::Sender<ConnectionStateSnapshot>) -> HttpFuture<Self> {
        // ignore send error, client might be already dead
        drop(sender.send(self.inner.with(|inner| inner.dump_state())));
        Box::new(future::finished(self))
    }

    fn process_message(self, message: ServerCommandMessage) -> HttpFuture<Self> {
        match message {
            ServerCommandMessage::DumpState(sender) => self.process_dump_state(sender),
        }
    }

    pub fn run(self, requests: HttpFutureStreamSend<ServerCommandMessage>) -> HttpFuture<()> {
        let requests = requests.map_err(error::Error::from);
        Box::new(requests
            .fold(self, move |l, message: ServerCommandMessage| {
                l.process_message(message)
            })
            .map(|_| ()))
    }}



pub struct ServerConnection {
    command_tx: UnboundedSender<ServerCommandMessage>,
}

impl ServerConnection {
    fn connected<F, I>(lh: &reactor::Handle, socket: HttpFutureSend<I>, cpu_pool: CpuPoolOption, conf: ServerConf, service: Arc<F>)
        -> (ServerConnection, HttpFuture<()>)
        where
            F : Service,
            I : AsyncRead + AsyncWrite + Send + 'static,
    {
        let lh = lh.clone();

        let (to_write_tx, to_write_rx) = unbounded::<ServerToWriteMessage>();
        let (command_tx, command_rx) = unbounded::<ServerCommandMessage>();

        let to_write_rx = to_write_rx.map_err(|()| error::Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write")));
        let command_rx = Box::new(command_rx.map_err(|()| error::Error::IoError(io::Error::new(io::ErrorKind::Other, "command"))));

        let settings_frame = SettingsFrame::from_settings(vec![ HttpSetting::EnablePush(false) ]);
        let mut settings = DEFAULT_SETTINGS;
        settings.apply_from_frame(&settings_frame);

        let handshake = socket.and_then(|conn| server_handshake(conn, settings_frame));

        let run = handshake.and_then(move |socket| {
            let (read, write) = socket.split();

            let inner = RcMut::new(ConnData::new(
                lh,
                cpu_pool,
                ServerConnData {
                    factory: service,
                },
                conf.common,
                settings,
                to_write_tx.clone()));

            let framed_read = HttpFrameJoinContinuationRead::new(read);

            let run_write = ServerWriteLoop { write, inner: inner.clone() }.run(Box::new(to_write_rx));
            let run_read = ServerReadLoop { framed_read, inner: inner.clone() }.run();
            let run_command = ServerCommandLoop { inner: inner.clone() }.run(command_rx);

            run_write.join(run_read).join(run_command).map(|_| ())
        });

        let future = Box::new(run.then(|x| { info!("connection end: {:?}", x); x }));

        (ServerConnection {
            command_tx: command_tx,
        }, future)
    }

    pub fn new<S, A>(
        lh: &reactor::Handle,
        socket: Box<StreamItem>,
        tls: ServerTlsOption<A>,
        exec: CpuPoolOption,
        conf: ServerConf, service: Arc<S>)
            -> (ServerConnection, HttpFuture<()>)
        where S : Service, A : TlsAcceptor
    {
        match tls {
            ServerTlsOption::Plain => {
                let socket = Box::new(future::finished(socket));
                ServerConnection::connected(lh, socket, exec, conf, service)
            }
            ServerTlsOption::Tls(acceptor) => {
                let socket = Box::new(
                    tokio_tls_api::accept_async(&*acceptor, socket).map_err(error::Error::from));
                ServerConnection::connected(lh, socket, exec, conf, service)
            }
        }
    }

    pub fn new_plain_single_thread<S>(lh: &reactor::Handle, socket: TcpStream, conf: ServerConf, service: Arc<S>)
        -> (ServerConnection, HttpFuture<()>)
        where
            S : Service,
    {
        let no_tls: ServerTlsOption<tls_api_stub::TlsAcceptor> = ServerTlsOption::Plain;
        ServerConnection::new(lh, Box::new(socket), no_tls, CpuPoolOption::SingleThread, conf, service)
    }

    pub fn new_plain_single_thread_fn<F>(lh: &reactor::Handle, socket: TcpStream, conf: ServerConf, f: F)
        -> (ServerConnection, HttpFuture<()>)
        where
            F : Fn(Headers, HttpStreamAfterHeaders) -> Response + Send + Sync + 'static,
    {
        struct HttpServiceFn<F>(F);

        impl<F> Service for HttpServiceFn<F>
            where F : Fn(Headers, HttpStreamAfterHeaders) -> Response + Send + Sync + 'static
        {
            fn start_request(&self, headers: Headers, req: HttpStreamAfterHeaders) -> Response {
                (self.0)(headers, req)
            }
        }

        ServerConnection::new_plain_single_thread(lh, socket, conf, Arc::new(HttpServiceFn(f)))
    }

    /// For tests
    pub fn dump_state(&self) -> HttpFutureSend<ConnectionStateSnapshot> {
        let (tx, rx) = oneshot::channel();

        if let Err(_) = self.command_tx.clone().unbounded_send(ServerCommandMessage::DumpState(tx)) {
            return Box::new(future::err(error::Error::Other("failed to send req to dump state")));
        }

        let rx = rx.map_err(|_| error::Error::Other("oneshot canceled"));

        Box::new(rx)
    }

}
