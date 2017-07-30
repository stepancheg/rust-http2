use std::sync::Arc;
use std::thread;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::oneshot;

use tokio_core::reactor;

use tls_api::TlsConnector;
use tls_api::TlsConnectorBuilder;
use tls_api_stub;

use futures_misc::*;

use error;
use error::Error;
use result::Result;

use solicit::header::*;
use solicit::HttpScheme;
use solicit::StreamId;

use solicit_async::*;

use client_conn::*;
use client_conf::*;
use common::*;
use stream_part::*;
use service::Service;
use socket::ToClientStream;

pub use client_tls::ClientTlsOption;


pub struct ClientBuilder<C : TlsConnector = tls_api_stub::TlsConnector,
                         T : ToClientStream = SocketAddr>
{
    pub event_loop: Option<reactor::Remote>,
    pub addr: Option<T>,
    pub tls: ClientTlsOption<C>,
    pub conf: ClientConf,
}

impl<T: ToClientStream + Send + Clone + 'static> ClientBuilder<tls_api_stub::TlsConnector, T> {
    pub fn new_plain() -> ClientBuilder<tls_api_stub::TlsConnector, T> {
        ClientBuilder::new()
    }
}

#[cfg(unix)]
impl ClientBuilder<tls_api_stub::TlsConnector, String> {
    pub fn new_plain_unix() -> ClientBuilder<tls_api_stub::TlsConnector, String> {
        ClientBuilder::<tls_api_stub::TlsConnector, String>::new()
    }
}

impl<C : TlsConnector> ClientBuilder<C, SocketAddr> {
    /// Set the addr client connects to.
    pub fn set_addr<S : ToSocketAddrs>(&mut self, addr: S) -> Result<()> {
        // TODO: sync
        let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(Error::Other("addr is resolved to empty list"));
        } else if addrs.len() > 1 {
            // TODO: allow multiple addresses
            return Err(Error::Other("addr is resolved to more than one addr"));
        }
        self.addr = Some(addrs.into_iter().next().unwrap());
        Ok(())
    }
}

#[cfg(unix)]
impl<C : TlsConnector> ClientBuilder<C, String> {
    /// Set the addr client connects to.
    pub fn set_unix_addr(&mut self, addr: &str) -> Result<()> {
        self.addr = Some(addr.to_owned());
        Ok(())
    }
}

impl<C : TlsConnector, T : ToClientStream + Send + Clone + 'static> ClientBuilder<C, T> {
    pub fn new() -> ClientBuilder<C, T> {
        ClientBuilder {
            event_loop: None,
            addr: None,
            tls: ClientTlsOption::Plain,
            conf: ClientConf::new(),
        }
    }

    pub fn set_tls(&mut self, host: &str) -> Result<()> {
        let mut tls_connector = C::builder()?;

        if C::supports_alpn() {
            // TODO: check negotiated protocol after connect
            tls_connector.set_alpn_protocols(&[b"h2"])?;
        }

        let tls_connector = tls_connector.build()?;

        let tls_connector = Arc::new(tls_connector);
        self.tls = ClientTlsOption::Tls(host.to_owned(), tls_connector);
        Ok(())
    }

    pub fn build(self) -> Result<Client> {
        let addr = self.addr.expect("addr is not specified");

        let http_scheme = self.tls.http_scheme();

        // Create a channel to receive shutdown signal.
        let (shutdown_signal, shutdown_future) = shutdown_signal();

        let (controller_tx, controller_rx) = unbounded();

        let (done_tx, done_rx) = oneshot::channel();

        let join = if let Some(remote) = self.event_loop {
            let tls = self.tls;
            let conf = self.conf;
            let controller_tx = controller_tx.clone();
            remote.spawn(move |handle| {
                spawn_client_event_loop(
                    handle.clone(),
                    shutdown_future,
                    addr,
                    tls,
                    conf,
                    done_tx,
                    controller_tx,
                    controller_rx);
                future::finished(())
            });
            Completion::Rx(done_rx)
        } else {
            // Start event loop.
            let tls = self.tls;
            let conf = self.conf;
            let thread_name = conf.thread_name.clone()
                .unwrap_or_else(|| "http2-client-loop".to_owned()).to_string();
            let controller_tx = controller_tx.clone();
            let join_handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    // Create an event loop.
                    let mut lp: reactor::Core = reactor::Core::new().expect("Core::new");

                    spawn_client_event_loop(
                        lp.handle(),
                        shutdown_future,
                        addr,
                        tls,
                        conf,
                        done_tx,
                        controller_tx,
                        controller_rx);

                    lp.run(done_rx).expect("run");
                })
                .expect("spawn");
            Completion::Thread(join_handle)
        };

        Ok(Client {
            join: Some(join),
            controller_tx: controller_tx,
            http_scheme: http_scheme,
            shutdown: shutdown_signal,
        })
    }
}


enum Completion {
    Thread(thread::JoinHandle<()>),
    Rx(oneshot::Receiver<()>),
}

pub struct Client {
    controller_tx: UnboundedSender<ControllerCommand>,
    join: Option<Completion>,
    http_scheme: HttpScheme,
    // used only once to send shutdown signal
    shutdown: ShutdownSignal,
}

impl Client {

    pub fn new_plain(host: &str, port: u16, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::new_plain();
        client.conf = conf;
        client.set_addr((host, port))?;
        client.build()
    }


    #[cfg(unix)]
    pub fn new_plain_unix(addr: &str, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::new_plain_unix();
        client.conf = conf;
        client.set_unix_addr(addr)?;
        client.build()
    }

    pub fn new_tls<C : TlsConnector>(host: &str, port: u16, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::<C, SocketAddr>::new();
        client.conf = conf;
        client.set_addr((host, port))?;
        client.set_tls(host)?;
        client.build()
    }

    pub fn new_expl<C : TlsConnector>(addr: &SocketAddr, tls: ClientTlsOption<C>, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::new();
        client.addr = Some(addr.clone());
        client.tls = tls;
        client.conf = conf;
        client.build()
    }

    pub fn start_request_simple(
        &self,
        headers: Headers,
        body: Bytes)
            -> Response
    {
        self.start_request(
            headers,
            HttpPartStream::once_bytes(body))
    }

    pub fn start_get(
        &self,
        path: &str,
        authority: &str)
            -> Response
    {
        let headers = Headers(vec![
            Header::new(":method", "GET"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_simple(headers, Bytes::new())
    }

    pub fn start_post(
        &self,
        path: &str,
        authority: &str,
        body: Bytes)
            -> Response
    {
        let headers = Headers(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_simple(headers, body)
    }

    pub fn dump_state(&self) -> HttpFutureSend<ConnectionStateSnapshot> {
        let (tx, rx) = oneshot::channel();
        // ignore error
        drop(self.controller_tx.send(ControllerCommand::DumpState(tx)));
        Box::new(rx.map_err(|_| error::Error::Other("conn died")))
    }

    pub fn wait_for_connect(&self) -> HttpFutureSend<()> {
        let (tx, rx) = oneshot::channel();
        // ignore error
        drop(self.controller_tx.send(ControllerCommand::WaitForConnect(tx)));
        Box::new(rx.map_err(|_| error::Error::Other("conn died")).and_then(|r| r))
    }
}

impl Service for Client {
    // TODO: copy-paste with ClientConnection::start_request
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

        if let Err(_) = self.controller_tx.send(ControllerCommand::StartRequest(start)) {
            return Response::err(error::Error::Other("client controller died"));
        }

        let resp_rx = resp_rx.map_err(|oneshot::Canceled| error::Error::Other("client likely died"));

        let resp_rx = resp_rx.map(|r| r.into_stream_flag());

        let resp_rx = resp_rx.flatten_stream();

        Response::from_stream(resp_rx)
    }}

enum ControllerCommand {
    GoAway,
    StartRequest(StartRequestMessage),
    WaitForConnect(oneshot::Sender<Result<()>>),
    DumpState(oneshot::Sender<ConnectionStateSnapshot>),
}

struct ControllerState<T : ToClientStream, C : TlsConnector> {
    handle: reactor::Handle,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    // current connection
    conn: Arc<ClientConnection>,
    tx: UnboundedSender<ControllerCommand>,
}

impl<T : ToClientStream + 'static + Clone, C : TlsConnector> ControllerState<T, C> {
    fn init_conn(&mut self) {
        let (conn, future) = ClientConnection::new(
            self.handle.clone(),
            Box::new(self.socket_addr.clone()),
            self.tls.clone(),
            self.conf.clone(),
            CallbacksImpl {
                tx: self.tx.clone(),
            });

        self.handle.spawn(future.map_err(|e| { warn!("client error: {:?}", e); () }));

        self.conn = Arc::new(conn);
    }

    fn iter(mut self, cmd: ControllerCommand) -> ControllerState<T, C> {
        match cmd {
            ControllerCommand::GoAway => {
                self.init_conn();
            },
            ControllerCommand::StartRequest(start) => {
                if let Err(start) = self.conn.start_request_with_resp_sender(start) {
                    self.init_conn();
                    if let Err(start) = self.conn.start_request_with_resp_sender(start) {
                        let err = error::Error::Other("client died and reconnect failed");
                        // ignore error
                        if let Err(_) = start.resp_tx.send(Response::err(err)) {
                            debug!("called likely died");
                        }
                    }
                }
            }
            ControllerCommand::WaitForConnect(tx) => {
                if let Err(tx) = self.conn.wait_for_connect_with_resp_sender(tx) {
                    self.init_conn();
                    if let Err(tx) = self.conn.wait_for_connect_with_resp_sender(tx) {
                        let err = error::Error::Other("client died and reconnect failed");
                        // ignore error
                        drop(tx.send(Err(err)));
                    }
                }
            }
            ControllerCommand::DumpState(tx) => {
                self.conn.dump_state_with_resp_sender(tx);
            }
        }
        self
    }

    fn run(self, rx: UnboundedReceiver<ControllerCommand>)
        -> HttpFuture<()>
    {
        let rx = rx.map_err(|_| error::Error::Other("channel died"));
        let r = rx.fold(self, |state, cmd| {
            Ok::<_, error::Error>(state.iter(cmd))
        });
        let r = r.map(|_| ());
        Box::new(r)
    }
}

struct CallbacksImpl {
    tx: UnboundedSender<ControllerCommand>,
}

impl ClientConnectionCallbacks for CallbacksImpl {
    fn goaway(&self, _stream_id: StreamId, _error_code: u32) {
        drop(self.tx.send(ControllerCommand::GoAway));
    }
}

// Event loop entry point
fn spawn_client_event_loop<T : ToClientStream + Send + Clone + 'static, C : TlsConnector>(
    handle: reactor::Handle,
    shutdown_future: ShutdownFuture,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    done_tx: oneshot::Sender<()>,
    controller_tx: UnboundedSender<ControllerCommand>,
    controller_rx: UnboundedReceiver<ControllerCommand>)
{
    let (http_conn, conn_future) =
        ClientConnection::new(handle.clone(), Box::new(socket_addr.clone()), tls.clone(), conf.clone(), CallbacksImpl {
            tx: controller_tx.clone(),
        });

    handle.spawn(conn_future.map_err(|e| { warn!("client error: {:?}", e); () }));

    let init = ControllerState {
        handle: handle.clone(),
        socket_addr: socket_addr.clone(),
        tls: tls,
        conf: conf,
        conn: Arc::new(http_conn),
        tx: controller_tx,
    };

    let controller_future = init.run(controller_rx);

    let shutdown_future = shutdown_future
        .then(move |_| {
            // Must complete with error,
            // so `join` with this future cancels another future.
            future::failed::<(), _>(Error::Shutdown)
        });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = controller_future.join(shutdown_future);

    let done = done.then(|_| {
        // OK to ignore error, because rx might be already dead
        drop(done_tx.send(()));
        info!("client stopped");
        Ok(())
    });

    handle.spawn(done);
}

// We shutdown the client in the destructor.
impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown.shutdown();

        // do not ignore errors of take
        // ignore errors of join, it means that server event loop crashed
        match self.join.take().unwrap() {
            Completion::Thread(join) => drop(join.join()),
            Completion::Rx(_rx) => {
                // cannot wait on _rx, because Core might not be running
            },
        };
    }
}
