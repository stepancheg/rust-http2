use std::io;
use std::sync::Arc;
use std::panic;

use error;
use result;

use solicit::StreamId;
use solicit::header::*;
use solicit::connection::EndStream;

use bytes::Bytes;

use futures;
use futures::Future;
use futures::stream;
use futures::stream::Stream;

use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_tls::TlsAcceptorExt;

use futures_misc::*;

use solicit_async::*;
use service::Service;
use stream_part::*;
use common::*;

use server_tls::*;
use server_conf::*;

use misc::any_to_string;
use rc_mut::*;


struct ServerTypes;

impl Types for ServerTypes {
    type HttpStream = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type ConnDataSpecific = ServerConnData;
    type ToWriteMessage = ServerToWriteMessage;

    fn first_id() -> StreamId {
        2
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
            let part = HttpStreamPart {
                content: HttpStreamPartContent::Headers(headers),
                last: last,
            };
            // ignore error
            sender.send(ResultOrEof::Item(part)).ok();
        }
    }
}

impl HttpStream for ServerStream {
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
    fn new_stream(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, headers: Headers)
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

        let (req_tx, req_rx) = futures::sync::mpsc::unbounded();

        let req_rx = req_rx.map_err(|()| error::Error::from(io::Error::new(io::ErrorKind::Other, "req")));
        let req_rx = stream_with_eof_and_error(req_rx, || error::Error::from(io::Error::new(io::ErrorKind::Other, "unexpected eof")));

        let response = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            self.specific.factory.start_request(headers, HttpPartStream::new(req_rx))
        }));

        let response = response.unwrap_or_else(|e| {
            let e = any_to_string(e);
            warn!("handler panicked: {}", e);

            let headers = Headers::internal_error_500();
            Response::from_stream(stream::iter(vec![
                Ok(HttpStreamPart::intermediate_headers(headers)),
                Ok(HttpStreamPart::last_data(Bytes::from(format!("handler panicked: {}", e)))),
            ]))
        });

        let (latch_ctr, latch) = latch();

        // TODO
        latch_ctr.open();

        {
            // New stream initiated by the client
            let stream = HttpStreamCommon::new(
                self.conn.peer_settings.initial_window_size,
                req_tx,
                latch_ctr,
                ServerStreamData {});
            self.streams.insert(stream_id, stream);
        }

        self.pump_stream_to_write_loop(self_rc, stream_id, response.into_part_stream(), latch);

        Ok(self.streams.get_mut(stream_id).expect("get stream"))
    }

    fn get_or_create_stream(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, headers: Headers, last: bool)
        -> result::Result<HttpStreamRef<ServerTypes>>
    {
        if self.streams.get_mut(stream_id).is_some() {
            // https://github.com/rust-lang/rust/issues/36403
            let mut stream = self.streams.get_mut(stream_id).unwrap();
            stream.stream().set_headers(headers, last);
            Ok(stream)
        } else {
            self.new_stream(self_rc, stream_id, headers)
        }
    }
}

impl ConnInner for ServerInner {
    type Types = ServerTypes;

    fn process_headers(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<ServerTypes>>>
    {
        let stream = self.get_or_create_stream(
            self_rc,
            stream_id,
            headers,
            end_stream == EndStream::Yes)?;

        Ok(Some(stream))
    }

    fn goaway_received(&mut self, _stream_id: StreamId, _raw_error_code: u32) {
        // ignore
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
    DumpState(futures::sync::oneshot::Sender<ConnectionStateSnapshot>),
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
    fn process_dump_state(self, sender: futures::sync::oneshot::Sender<ConnectionStateSnapshot>) -> HttpFuture<Self> {
        // ignore send error, client might be already dead
        drop(sender.send(self.inner.with(|inner| inner.dump_state())));
        Box::new(futures::finished(self))
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
    command_tx: futures::sync::mpsc::UnboundedSender<ServerCommandMessage>,
}

impl ServerConnection {
    fn connected<F, I>(lh: &reactor::Handle, socket: HttpFutureSend<I>, conf: ServerConf, service: Arc<F>)
                       -> (ServerConnection, HttpFuture<()>)
        where
            F : Service,
            I : AsyncRead + AsyncWrite + Send + 'static,
    {
        let lh = lh.clone();

        let (to_write_tx, to_write_rx) = futures::sync::mpsc::unbounded::<ServerToWriteMessage>();
        let (command_tx, command_rx) = futures::sync::mpsc::unbounded::<ServerCommandMessage>();

        let to_write_rx = to_write_rx.map_err(|()| error::Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write")));
        let command_rx = Box::new(command_rx.map_err(|()| error::Error::IoError(io::Error::new(io::ErrorKind::Other, "command"))));

        let handshake = socket.and_then(server_handshake);

        let run = handshake.and_then(move |socket| {
            let (read, write) = socket.split();

            let inner = RcMut::new(ConnData::new(
                lh,
                ServerConnData {
                    factory: service,
                },
                conf.common,
                to_write_tx.clone()));

            let run_write = ServerWriteLoop { write: write, inner: inner.clone() }.run(Box::new(to_write_rx));
            let run_read = ServerReadLoop { read: read, inner: inner.clone() }.run();
            let run_command = ServerCommandLoop { inner: inner.clone() }.run(command_rx);

            run_write.join(run_read).join(run_command).map(|_| ())
        });

        let future = Box::new(run.then(|x| { info!("connection end: {:?}", x); x }));

        (ServerConnection {
            command_tx: command_tx,
        }, future)
    }

    pub fn new<S>(lh: &reactor::Handle, socket: TcpStream, tls: ServerTlsOption, conf: ServerConf, service: Arc<S>)
                  -> (ServerConnection, HttpFuture<()>)
        where
            S : Service,
    {
        match tls {
            ServerTlsOption::Plain =>
                ServerConnection::connected(
                    lh, Box::new(futures::finished(socket)), conf, service),
            ServerTlsOption::Tls(acceptor) =>
                ServerConnection::connected(
                    lh, Box::new(acceptor.accept_async(socket).map_err(error::Error::from)), conf, service),
        }
    }

    pub fn new_plain<S>(lh: &reactor::Handle, socket: TcpStream, conf: ServerConf, service: Arc<S>)
                        -> (ServerConnection, HttpFuture<()>)
        where
            S : Service,
    {
        ServerConnection::new(lh, socket, ServerTlsOption::Plain, conf, service)
    }

    pub fn new_plain_fn<F>(lh: &reactor::Handle, socket: TcpStream, conf: ServerConf, f: F)
                           -> (ServerConnection, HttpFuture<()>)
        where
            F : Fn(Headers, HttpPartStream) -> Response + Send + 'static,
    {
        struct HttpServiceFn<F>(F);

        impl<F> Service for HttpServiceFn<F>
            where F : Fn(Headers, HttpPartStream) -> Response + Send + 'static
        {
            fn start_request(&self, headers: Headers, req: HttpPartStream) -> Response {
                (self.0)(headers, req)
            }
        }

        ServerConnection::new_plain(lh, socket, conf, Arc::new(HttpServiceFn(f)))
    }

    /// For tests
    pub fn dump_state(&self) -> HttpFutureSend<ConnectionStateSnapshot> {
        let (tx, rx) = futures::oneshot();

        self.command_tx.clone().send(ServerCommandMessage::DumpState(tx))
            .expect("send request to dump state");

        let rx = rx.map_err(|_| error::Error::Other("oneshot canceled"));

        Box::new(rx)
    }

}
