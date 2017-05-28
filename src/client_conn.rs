//! Single client connection

use std::result::Result as std_Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::io;

use error;
use error::ErrorCode;
use error::Error;
use result;

use solicit::StreamId;
use solicit::header::*;
use solicit::connection::EndStream;

use service::Service;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::oneshot;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;

use native_tls::TlsConnector;

use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_timer::Timer;
use tokio_io::AsyncWrite;
use tokio_io::AsyncRead;
use tokio_tls::TlsConnectorExt;

use futures_misc::*;

use solicit_async::*;

use common::*;
use stream_part::*;
use client_conf::*;
use client_tls::*;


struct ClientTypes;

impl Types for ClientTypes {
    type HttpStream = ClientStream;
    type HttpStreamSpecific = ClientStreamData;
    type ConnDataSpecific = ClientConnData;
    type ConnData = ClientInner;
    type ToWriteMessage = ClientToWriteMessage;

    fn first_id() -> StreamId {
        1
    }
}



pub struct ClientStreamData {
}

impl HttpStreamDataSpecific for ClientStreamData {
}

type ClientStream = HttpStreamCommon<ClientTypes>;

impl HttpStream for ClientStream {
    type Types = ClientTypes;
}

pub struct ClientConnData {
    callbacks: Box<ClientConnectionCallbacks>,
}

impl ConnDataSpecific for ClientConnData {
}

type ClientInner = ConnData<ClientTypes>;

impl ClientInner {
    fn insert_stream(&mut self, stream: ClientStream) -> StreamId {
        let id = self.next_local_stream_id();
        self.streams.insert(id, stream);
        id
    }
}

impl ConnInner for ClientInner {
    type Types = ClientTypes;

    fn process_headers(&mut self, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<ClientTypes>>>
    {
        let mut stream: HttpStreamRef<ClientTypes> = match self.streams.get_mut(stream_id) {
            None => {
                // TODO: send stream closed
                return Err(error::Error::Other("??"));
            }
            Some(stream) => stream,
        };
        // TODO: hack
        if headers.0.len() != 0 {

            if let Some(ref mut response_handler) = stream.stream().peer_tx {
                // TODO: reset stream if called is dead
                drop(response_handler.send(ResultOrEof::Item(HttpStreamPart {
                    content: HttpStreamPartContent::Headers(headers),
                    last: end_stream == EndStream::Yes,
                })));
            }
        }

        Ok(Some(stream))
    }

    fn goaway_received(&mut self, stream_id: StreamId, raw_error_code: u32) {
        self.specific.callbacks.goaway(stream_id, raw_error_code);
    }
}

pub struct ClientConnection {
    write_tx: UnboundedSender<ClientToWriteMessage>,
    command_tx: UnboundedSender<ClientCommandMessage>,
}

unsafe impl Sync for ClientConnection {}

pub struct StartRequestMessage {
    pub headers: Headers,
    pub body: HttpPartStream,
    pub resp_tx: UnboundedSender<ResultOrEof<HttpStreamPart, Error>>,
}

struct BodyChunkMessage {
    stream_id: StreamId,
    chunk: Bytes,
}

struct EndRequestMessage {
    stream_id: StreamId,
}

enum ClientToWriteMessage {
    Start(StartRequestMessage),
    BodyChunk(BodyChunkMessage),
    End(EndRequestMessage),
    Common(CommonToWriteMessage),
}

impl From<CommonToWriteMessage> for ClientToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ClientToWriteMessage::Common(m)
    }
}

enum ClientCommandMessage {
    DumpState(oneshot::Sender<ConnectionStateSnapshot>),
    WaitForHandshake(oneshot::Sender<result::Result<()>>),
}


impl<I : AsyncWrite + Send + 'static> ClientWriteLoop<I> {
    fn process_start(self, start: StartRequestMessage) -> HttpFuture<Self> {
        let StartRequestMessage { headers, body, resp_tx } = start;

        let stream_id = self.inner.with(move |inner: &mut ClientInner| {

            let mut stream = HttpStreamCommon::new(
                inner.conn.peer_settings.initial_window_size,
                resp_tx,
                ClientStreamData { });

            stream.outgoing.push_back(HttpStreamPartContent::Headers(headers));

            let stream_id = inner.insert_stream(stream);
            let to_write_tx_1 = inner.to_write_tx.clone();
            let to_write_tx_2 = inner.to_write_tx.clone();

            let future = body
                .check_only_data() // TODO: headers too
                .fold((), move |(), chunk| {
                    future::result(to_write_tx_1.send(ClientToWriteMessage::BodyChunk(BodyChunkMessage {
                        stream_id: stream_id,
                        chunk: chunk,
                    })).map_err(|_| error::Error::Other("client must be dead")))
                });
            let future = future
                .and_then(move |()| {
                    future::result(to_write_tx_2.send(ClientToWriteMessage::End(EndRequestMessage {
                        stream_id: stream_id,
                    })).map_err(|_| error::Error::Other("client must be dead")))
                });

            let future = future.map_err(|e| {
                // TODO: should probably close-local or RST_STREAM the request
                warn!("call handle future is dead: {:?}", e);
                ()
            });

            inner.loop_handle.spawn(future);

            stream_id
        });

        self.send_outg_stream(stream_id)
    }

    fn process_body_chunk(self, body_chunk: BodyChunkMessage) -> HttpFuture<Self> {
        let BodyChunkMessage { stream_id, chunk } = body_chunk;

        self.inner.with(move |inner: &mut ClientInner| {
            let mut stream = inner.streams.get_mut(stream_id)
                .expect(&format!("stream not found: {}", stream_id));
            // TODO: check stream state

            stream.stream().outgoing.push_back(HttpStreamPartContent::Data(Bytes::from(chunk)));
        });

        self.send_outg_stream(stream_id)
    }

    fn process_end(self, end: EndRequestMessage) -> HttpFuture<Self> {
        let EndRequestMessage { stream_id } = end;

        self.inner.with(move |inner: &mut ClientInner| {
            // TODO: do not panic
            let mut stream = inner.streams.get_mut(stream_id)
                .expect(&format!("stream not found: {}", stream_id));

            // TODO: check stream state
            stream.stream().outgoing_end = Some(ErrorCode::NoError);
        });

        self.send_outg_stream(stream_id)
    }

    fn process_message(self, message: ClientToWriteMessage) -> HttpFuture<Self> {
        match message {
            ClientToWriteMessage::Start(start) => self.process_start(start),
            ClientToWriteMessage::BodyChunk(body_chunk) => self.process_body_chunk(body_chunk),
            ClientToWriteMessage::End(end) => self.process_end(end),
            ClientToWriteMessage::Common(common) => self.process_common(common),
        }
    }

    pub fn run(self, requests: HttpFutureStreamSend<ClientToWriteMessage>) -> HttpFuture<()> {
        let requests = requests.map_err(Error::from);
        Box::new(requests
            .fold(self, move |wl, message: ClientToWriteMessage| {
                wl.process_message(message)
            })
            .map(|_| ()))
    }
}

type ClientReadLoop<I> = ReadLoopData<I, ClientTypes>;
type ClientWriteLoop<I> = WriteLoopData<I, ClientTypes>;
type ClientCommandLoop = CommandLoopData<ClientTypes>;


pub trait ClientConnectionCallbacks : 'static {
    // called at most once
    fn goaway(&self, stream_id: StreamId, raw_error_code: u32);
}


impl ClientConnection {
    fn connected<I, C>(
        lh: reactor::Handle, connect: HttpFutureSend<I>,
        _conf: ClientConf,
        callbacks: C)
            -> (Self, HttpFuture<()>)
        where
            I : AsyncWrite + AsyncRead + Send + 'static,
            C : ClientConnectionCallbacks,
    {
        let (to_write_tx, to_write_rx) = unbounded();
        let (command_tx, command_rx) = unbounded();

        let to_write_rx = Box::new(to_write_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write"))));
        let command_rx = Box::new(command_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write"))));

        let c = ClientConnection {
            write_tx: to_write_tx.clone(),
            command_tx: command_tx,
        };

        let handshake = connect.and_then(client_handshake);

        let future = handshake.and_then(move |conn| {
            debug!("handshake done");
            let (read, write) = conn.split();

            let inner = TaskRcMut::new(ConnData::new(
                lh,
                ClientConnData {
                    callbacks: Box::new(callbacks),
                },
                to_write_tx.clone()));

            let run_write = ClientWriteLoop { write: write, inner: inner.clone() }.run(to_write_rx);
            let run_read = ClientReadLoop { read: read, inner: inner.clone() }.run();
            let run_command = ClientCommandLoop { inner: inner.clone() }.run(command_rx);

            run_write.join(run_read).join(run_command).map(|_| ())
        });

        (c, Box::new(future))
    }

    pub fn new<C>(
        lh: reactor::Handle,
        addr: &SocketAddr,
        tls: ClientTlsOption,
        conf: ClientConf,
        callbacks: C)
            -> (Self, HttpFuture<()>)
        where C : ClientConnectionCallbacks
    {
        match tls {
            ClientTlsOption::Plain =>
                ClientConnection::new_plain(lh, addr, conf, callbacks),
            ClientTlsOption::Tls(domain, connector) =>
                ClientConnection::new_tls(lh, &domain, connector, addr, conf, callbacks),
        }
    }

    pub fn new_plain<C>(
        lh: reactor::Handle,
        addr: &SocketAddr,
        conf: ClientConf,
        callbacks: C)
            -> (Self, HttpFuture<()>)
        where C : ClientConnectionCallbacks
    {
        let addr = addr.clone();

        let no_delay = conf.no_delay.unwrap_or(true);
        let connect = TcpStream::connect(&addr, &lh).map_err(Into::into);
        let map_callback = move |socket: TcpStream| {
            info!("connected to {}", addr);

            socket.set_nodelay(no_delay).expect("failed to set TCP_NODELAY");

            socket
        };

        let connect = if let Some(timeout) = conf.connection_timeout {
            let timer = Timer::default();
            timer.timeout(connect, timeout).map(map_callback).boxed()
        } else {
            connect.map(map_callback).boxed()
        };

        ClientConnection::connected(lh, connect, conf, callbacks)
    }

    pub fn new_tls<C>(
        lh: reactor::Handle,
        domain: &str,
        connector: Arc<TlsConnector>,
        addr: &SocketAddr,
        conf: ClientConf,
        callbacks: C)
            -> (Self, HttpFuture<()>)
        where C : ClientConnectionCallbacks
    {
        let domain = domain.to_owned();
        let addr = addr.clone();

        let connect = TcpStream::connect(&addr, &lh)
            .map(move |c| { info!("connected to {}", addr); c })
            .map_err(|e| e.into());

        let tls_conn = connect.and_then(move |conn| {
            connector.connect_async(&domain, conn).map_err(|e| {
                Error::IoError(io::Error::new(io::ErrorKind::Other, e))
            })
        });

        let tls_conn = tls_conn.map_err(Error::from);

        ClientConnection::connected(lh, Box::new(tls_conn), conf, callbacks)
    }

    pub fn start_request_with_resp_sender(
        &self,
        start: StartRequestMessage)
            -> Result<(), StartRequestMessage>
    {
        self.write_tx.send(ClientToWriteMessage::Start(start))
            .map_err(|send_error| {
                match send_error.into_inner() {
                    ClientToWriteMessage::Start(start) => start,
                    _ => unreachable!(),
                }
            })
    }

    pub fn dump_state_with_resp_sender(&self, tx: oneshot::Sender<ConnectionStateSnapshot>) {
        // ignore error
        drop(self.command_tx.send(ClientCommandMessage::DumpState(tx)));
    }

    /// For tests
    pub fn dump_state(&self) -> HttpFutureSend<ConnectionStateSnapshot> {
        let (tx, rx) = oneshot::channel();

        self.dump_state_with_resp_sender(tx);

        let rx = rx.map_err(|_| Error::from(io::Error::new(io::ErrorKind::Other, "oneshot canceled")));

        Box::new(rx)
    }

    pub fn wait_for_connect_with_resp_sender(&self, tx: oneshot::Sender<result::Result<()>>)
        -> std_Result<(), oneshot::Sender<result::Result<()>>>
    {
        self.command_tx.send(ClientCommandMessage::WaitForHandshake(tx))
            .map_err(|send_error| {
                match send_error.into_inner() {
                    ClientCommandMessage::WaitForHandshake(tx) => tx,
                    _ => unreachable!(),
                }
            })
    }
}

impl Service for ClientConnection {
    fn start_request(
        &self,
        headers: Headers,
        body: HttpPartStream)
            -> Response
    {
        let (resp_tx, resp_rx) = unbounded();

        let start = StartRequestMessage {
            headers: headers,
            body: body,
            resp_tx: resp_tx,
        };

        if let Err(_) = self.start_request_with_resp_sender(start) {
            return Response::err(error::Error::Other("client died"));
        }

        let req_rx = resp_rx.map_err(|()| Error::from(io::Error::new(io::ErrorKind::Other, "req")));

        let req_rx = stream_with_eof_and_error(req_rx, || error::Error::Other("client is likely died"));

        Response::from_stream(req_rx)
    }
}

impl ClientCommandLoop {
    fn process_dump_state(self, sender: oneshot::Sender<ConnectionStateSnapshot>) -> HttpFuture<Self> {
        // ignore send error, client might be already dead
        drop(sender.send(self.inner.with(|inner| inner.dump_state())));
        Box::new(future::finished(self))
    }

    fn process_message(self, message: ClientCommandMessage) -> HttpFuture<Self> {
        match message {
            ClientCommandMessage::DumpState(sender) => self.process_dump_state(sender),
            ClientCommandMessage::WaitForHandshake(tx) => {
                // ignore error
                drop(tx.send(Ok(())));
                Box::new(future::ok(self))
            },
        }
    }

    fn run(self, requests: HttpFutureStreamSend<ClientCommandMessage>) -> HttpFuture<()> {
        let requests = requests.map_err(Error::from);
        Box::new(requests
            .fold(self, move |l, message: ClientCommandMessage| {
                l.process_message(message)
            })
            .map(|_| ()))
    }
}
