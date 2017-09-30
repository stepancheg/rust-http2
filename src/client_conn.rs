//! Single client connection

use std::result::Result as std_Result;
use std::sync::Arc;
use std::io;

use error;
use error::Error;
use result;

use exec::CpuPoolOption;

use solicit::StreamId;
use solicit::header::*;
use solicit::connection::EndStream;
use solicit::frame::settings::*;
use solicit::DEFAULT_SETTINGS;

use service::Service;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::oneshot;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;

use tls_api::TlsConnector;

use tokio_core::reactor;
use tokio_timer::Timer;
use tokio_io::AsyncWrite;
use tokio_io::AsyncRead;
use tokio_tls_api;

use futures_misc::*;

use solicit_async::*;

use common::*;
use stream_part::*;
use client_conf::*;
use client_tls::*;
use socket::*;

use rc_mut::*;


struct ClientTypes;

impl Types for ClientTypes {
    type HttpStream = ClientStream;
    type HttpStreamSpecific = ClientStreamData;
    type ConnDataSpecific = ClientConnData;
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

impl ConnInner for ClientInner {
    type Types = ClientTypes;

    fn process_headers(&mut self, _self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<ClientTypes>>>
    {
        if let Some(mut stream) = self.get_stream_or_send_stream_closed(stream_id)? {
            if let Some(ref mut response_handler) = stream.stream().peer_tx {
                // TODO: reset stream on error
                drop(response_handler.send(ResultOrEof::Item(HttpStreamPart {
                    content: HttpStreamPartContent::Headers(headers),
                    last: end_stream == EndStream::Yes,
                })));
            } else {
                // TODO: reset stream
            }

            Ok(Some(stream))
        } else {
            Ok(None)
        }
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
    pub resp_tx: oneshot::Sender<Response>,
}

enum ClientToWriteMessage {
    Start(StartRequestMessage),
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

            let stream_id = inner.next_local_stream_id();

            let out_window = {
                let (mut http_stream, resp_stream, out_window) = inner.new_stream_data(
                    stream_id,
                    ClientStreamData { });

                if let Err(_) = resp_tx.send(Response::from_stream(resp_stream)) {
                    warn!("caller died");
                }

                http_stream.stream().outgoing.push_back(HttpStreamPartContent::Headers(headers));

                out_window
            };

            inner.pump_stream_to_write_loop(stream_id, body, out_window);

            stream_id
        });

        // Also opens latch if necessary
        self.send_outg_stream(stream_id)
    }

    fn process_message(self, message: ClientToWriteMessage) -> HttpFuture<Self> {
        match message {
            ClientToWriteMessage::Start(start) => self.process_start(start),
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
        conf: ClientConf,
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

        let settings_frame = SettingsFrame::from_settings(vec![ HttpSetting::EnablePush(false) ]);
        let mut settings = DEFAULT_SETTINGS;
        settings.apply_from_frame(&settings_frame);

        let handshake = connect.and_then(|conn| client_handshake(conn, settings_frame));

        let future = handshake.and_then(move |conn| {
            debug!("handshake done");
            let (read, write) = conn.split();

            let inner = RcMut::new(ConnData::new(
                lh,
                CpuPoolOption::SingleThread,
                ClientConnData {
                    callbacks: Box::new(callbacks),
                },
                conf.common,
                settings,
                to_write_tx.clone()));

            let run_write = ClientWriteLoop { write: write, inner: inner.clone() }.run(to_write_rx);
            let run_read = ClientReadLoop { read: read, inner: inner.clone() }.run();
            let run_command = ClientCommandLoop { inner: inner.clone() }.run(command_rx);

            run_write.join(run_read).join(run_command).map(|_| ())
        });

        (c, Box::new(future))
    }

    pub fn new<H, C>(
        lh: reactor::Handle,
        addr: Box<ToClientStream>,
        tls: ClientTlsOption<C>,
        conf: ClientConf,
        callbacks: H)
            -> (Self, HttpFuture<()>)
        where H : ClientConnectionCallbacks, C : TlsConnector + Sync
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
        addr: Box<ToClientStream>,
        conf: ClientConf,
        callbacks: C)
            -> (Self, HttpFuture<()>)
        where C : ClientConnectionCallbacks
    {
        let no_delay = conf.no_delay.unwrap_or(true);
        let connect = addr.connect(&lh).map_err(Into::into);
        let map_callback = move |socket: Box<StreamItem>| {
            info!("connected to {}", addr);

            if socket.is_tcp() {
                socket.set_nodelay(no_delay).expect("failed to set TCP_NODELAY");
            }

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

    pub fn new_tls<H, C>(
        lh: reactor::Handle,
        domain: &str,
        connector: Arc<C>,
        addr: Box<ToClientStream>,
        conf: ClientConf,
        callbacks: H)
            -> (Self, HttpFuture<()>)
        where H : ClientConnectionCallbacks, C : TlsConnector + Sync
    {
        let domain = domain.to_owned();

        let connect = addr.connect(&lh)
            .map(move |c| { info!("connected to {}", addr); c })
            .map_err(|e| e.into());

        let tls_conn = connect.and_then(move |conn| {
            tokio_tls_api::connect_async(&*connector, &domain, conn).map_err(|e| {
                Error::IoError(io::Error::new(io::ErrorKind::Other, format!("TLS connection failed {:?}", e)))
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
    // TODO: copy-paste with Client::start_request
    fn start_request(
        &self,
        headers: Headers,
        body: HttpPartStream)
            -> Response
    {
        let (resp_tx, resp_rx) = oneshot::channel();

        let start = StartRequestMessage {
            headers: headers,
            body: body,
            resp_tx: resp_tx,
        };

        if let Err(_) = self.start_request_with_resp_sender(start) {
            return Response::err(error::Error::Other("client died"));
        }

        let resp_rx = resp_rx.map_err(|e| error::Error::InvalidFrame(format!("client likely died {:?}", e)));

        let resp_rx = resp_rx.map(|r| r.into_stream_flag());

        let resp_rx = resp_rx.flatten_stream();

        Response::from_stream(resp_rx)
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
