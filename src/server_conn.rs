use std::io;
use std::sync::Arc;
use std::panic;

use error::ErrorCode;
use error::Error;
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


struct ServerTypes;

impl Types for ServerTypes {
    type HttpStream = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type ConnDataSpecific = ServerConnData;
    type ConnData = ServerInner;
    type ToWriteMessage = ServerToWriteMessage;

    fn first_id() -> StreamId {
        2
    }
}


pub struct ServerStreamData {
    request_handler: Option<futures::sync::mpsc::UnboundedSender<ResultOrEof<HttpStreamPart, Error>>>,
}

impl HttpStreamDataSpecific for ServerStreamData {
}

type ServerStream = HttpStreamCommon<ServerTypes>;

impl ServerStream {
    fn set_headers(&mut self, headers: Headers, last: bool) {
        if let Some(ref mut sender) = self.specific.request_handler {
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

    fn new_data_chunk(&mut self, data: &[u8], last: bool) {
        if let Some(ref mut sender) = self.specific.request_handler {
            let part = HttpStreamPart {
                content: HttpStreamPartContent::Data(Bytes::from(data)),
                last: last,
            };
            // ignore error
            sender.send(ResultOrEof::Item(part)).ok();
        }
    }

    fn rst(&mut self, error_code: ErrorCode) {
        if let Some(ref mut sender) = self.specific.request_handler {
            // ignore error
            sender.send(ResultOrEof::Error(Error::CodeError(error_code))).ok();
        }
    }

    fn closed_remote(&mut self) {
        if let Some(sender) = self.specific.request_handler.take() {
            // ignore error
            sender.send(ResultOrEof::Eof).ok();
        }
    }
}

struct ServerConnData {
    factory: Arc<Service>,
}

impl ConnDataSpecific for ServerConnData {
}

type ServerInner = ConnData<ServerTypes>;

impl ServerInner {
    fn new_request(&mut self, stream_id: StreamId, headers: Headers)
        -> futures::sync::mpsc::UnboundedSender<ResultOrEof<HttpStreamPart, Error>>
    {
        let (req_tx, req_rx) = futures::sync::mpsc::unbounded();

        let req_rx = req_rx.map_err(|()| Error::from(io::Error::new(io::ErrorKind::Other, "req")));
        let req_rx = stream_with_eof_and_error(req_rx, || Error::from(io::Error::new(io::ErrorKind::Other, "unexpected eof")));

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

        let response = panic::AssertUnwindSafe(response.into_stream_flag()).catch_unwind().then(|r| {
            match r {
                Ok(r) => r,
                Err(e) => {
                    let e = any_to_string(e);
                    // TODO: send plain text error if headers weren't sent yet
                    warn!("handler panicked: {}", e);
                    Err(Error::HandlerPanicked(e))
                },
            }
        });

        {
            let to_write_tx1 = self.to_write_tx.clone();
            let to_write_tx2 = to_write_tx1.clone();

            let process_response = response.for_each(move |part: HttpStreamPart| {
                // drop error if connection is closed
                if let Err(e) = to_write_tx1.send(ServerToWriteMessage::ResponsePart(stream_id, part)) {
                    warn!("failed to write to channel, probably connection is closed: {}", e);
                }
                Ok(())
            }).then(move |r| {
                let error_code =
                    match r {
                        Ok(()) => ErrorCode::NoError,
                        Err(e) => {
                            warn!("handler stream error: {:?}", e);
                            ErrorCode::InternalError
                        }
                    };
                if let Err(e) = to_write_tx2.send(ServerToWriteMessage::ResponseStreamEnd(stream_id, error_code)) {
                    warn!("failed to write to channel, probably connection is closed: {}", e);
                }
                Ok(())
            });

            self.loop_handle.spawn(process_response);
        }

        req_tx
    }

    fn new_stream(&mut self, stream_id: StreamId, headers: Headers) -> &mut ServerStream {
        debug!("new stream: {}", stream_id);

        let req_tx = self.new_request(stream_id, headers);

        // New stream initiated by the client
        let stream = HttpStreamCommon::new(
            self.conn.peer_settings.initial_window_size,
            ServerStreamData {
                request_handler: Some(req_tx)
            });
        if let Some(..) = self.streams.map.insert(stream_id, stream) {
            panic!("inserted stream that already existed");
        }
        self.streams.map.get_mut(&stream_id).unwrap()
    }

    fn get_or_create_stream(&mut self, stream_id: StreamId, headers: Headers, last: bool) -> &mut ServerStream {
        if self.streams.get_mut(stream_id).is_some() {
            // https://github.com/rust-lang/rust/issues/36403
            let stream = self.streams.get_mut(stream_id).unwrap();
            stream.set_headers(headers, last);
            stream
        } else {
            self.new_stream(stream_id, headers)
        }
    }
}

impl ConnInner for ServerInner {
    type Types = ServerTypes;

    fn process_headers(&mut self, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<()>
    {
        let _stream = self.get_or_create_stream(
            stream_id,
            headers,
            end_stream == EndStream::Yes);

        // TODO: drop stream if closed on both ends

        Ok(())
    }

    fn goaway_received(&mut self, _stream_id: StreamId, _raw_error_code: u32) {
        // ignore
    }
}

type ServerReadLoop<I> = ReadLoopData<I, ServerTypes>;
type ServerWriteLoop<I> = WriteLoopData<I, ServerTypes>;
type ServerCommandLoop = CommandLoopData<ServerTypes>;


enum ServerToWriteMessage {
    ResponsePart(StreamId, HttpStreamPart),
    // send when user provided handler completed the stream
    ResponseStreamEnd(StreamId, ErrorCode),
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

    fn process_response_part(self, stream_id: StreamId, part: HttpStreamPart) -> HttpFuture<Self> {
        let stream_id = self.inner.with(move |inner: &mut ServerInner| {
            let stream = inner.streams.get_mut(stream_id);
            if let Some(stream) = stream {
                if !stream.state.is_closed_local() {
                    stream.outgoing.push_back(part.content);
                    if part.last {
                        stream.outgoing_end = Some(ErrorCode::NoError);
                    }
                    Some(stream_id)
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(stream_id) = stream_id {
            self.send_outg_stream(stream_id)
        } else {
            Box::new(futures::finished(self))
        }
    }

    fn process_response_end(self, stream_id: StreamId, error_code: ErrorCode) -> HttpFuture<Self> {
        let stream_id = self.inner.with(move |inner: &mut ServerInner| {
            let stream = inner.streams.get_mut(stream_id);
            if let Some(stream) = stream {
                if stream.outgoing_end.is_none() {
                    stream.outgoing_end = Some(error_code);
                }
                Some(stream_id)
            } else {
                None
            }
        });
        if let Some(stream_id) = stream_id {
            self.send_outg_stream(stream_id)
        } else {
            Box::new(futures::finished(self))
        }
    }

    fn process_message(self, message: ServerToWriteMessage) -> HttpFuture<Self> {
        match message {
            ServerToWriteMessage::ResponsePart(stream_id, response) => {
                self.process_response_part(stream_id, response)
            },
            ServerToWriteMessage::ResponseStreamEnd(stream_id, error_code) => {
                self.process_response_end(stream_id, error_code)
            },
            ServerToWriteMessage::Common(common) => {
                self.process_common(common)
            },
        }
    }

    fn run(self, requests: HttpFutureStream<ServerToWriteMessage>) -> HttpFuture<()> {
        let requests = requests.map_err(Error::from);
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
        let requests = requests.map_err(Error::from);
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
    fn connected<F, I>(lh: &reactor::Handle, socket: HttpFutureSend<I>, _conf: ServerConf, service: Arc<F>)
                       -> (ServerConnection, HttpFuture<()>)
        where
            F : Service,
            I : AsyncRead + AsyncWrite + Send + 'static,
    {
        let lh = lh.clone();

        let (to_write_tx, to_write_rx) = futures::sync::mpsc::unbounded::<ServerToWriteMessage>();
        let (command_tx, command_rx) = futures::sync::mpsc::unbounded::<ServerCommandMessage>();

        let to_write_rx = to_write_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write")));
        let command_rx = Box::new(command_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "command"))));

        let handshake = socket.and_then(server_handshake);

        let run = handshake.and_then(move |socket| {
            let (read, write) = socket.split();

            let inner = TaskRcMut::new(ConnData::new(
                lh,
                ServerConnData {
                    factory: service,
                },
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
                    lh, Box::new(acceptor.accept_async(socket).map_err(Error::from)), conf, service),
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

        let rx = rx.map_err(|_| Error::from(io::Error::new(io::ErrorKind::Other, "oneshot canceled")));

        Box::new(rx)
    }

}
