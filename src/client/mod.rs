pub(crate) mod conf;
pub(crate) mod conn;
pub(crate) mod req;
pub(crate) mod stream_handler;
pub(crate) mod tls;
pub(crate) mod types;

use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
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

use solicit_async::*;

use socket::AnySocketAddr;
use socket::ToClientStream;

use client::conf::ClientConf;
use client::conn::ClientConn;
use client::conn::ClientConnCallbacks;
use client::conn::StartRequestMessage;
use client::req::ClientRequest;
pub use client::tls::ClientTlsOption;
use client_died_error_holder::ClientDiedErrorHolder;
use client_died_error_holder::ClientDiedType;
use common::conn::ConnStateSnapshot;
use solicit::stream_id::StreamId;
use Response;

/// Builder for HTTP/2 client.
///
/// Client parameters can be specified only during construction,
/// and later client cannot be reconfigured.
pub struct ClientBuilder<C: TlsConnector = tls_api_stub::TlsConnector> {
    pub event_loop: Option<reactor::Remote>,
    pub addr: Option<AnySocketAddr>,
    pub tls: ClientTlsOption<C>,
    pub conf: ClientConf,
}

impl ClientBuilder<tls_api_stub::TlsConnector> {
    pub fn new_plain() -> ClientBuilder<tls_api_stub::TlsConnector> {
        ClientBuilder::new()
    }
}

impl<C: TlsConnector> ClientBuilder<C> {
    /// Set the addr client connects to.
    pub fn set_addr<S: ToSocketAddrs>(&mut self, addr: S) -> Result<()> {
        // TODO: sync
        let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(Error::Other("addr is resolved to empty list"));
        } else if addrs.len() > 1 {
            // TODO: allow multiple addresses
            return Err(Error::Other("addr is resolved to more than one addr"));
        }
        self.addr = Some(AnySocketAddr::Inet(addrs.into_iter().next().unwrap()));
        Ok(())
    }
}

#[cfg(unix)]
impl<C: TlsConnector> ClientBuilder<C> {
    /// Set the addr client connects to.
    pub fn set_unix_addr(&mut self, addr: &str) -> Result<()> {
        self.addr = Some(AnySocketAddr::Unix(addr.to_owned()));
        Ok(())
    }
}

impl<C: TlsConnector> ClientBuilder<C> {
    pub fn new() -> ClientBuilder<C> {
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

        let client_died_error_holder = ClientDiedErrorHolder::new();
        let client_died_error_holder_copy = client_died_error_holder.clone();

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
                    controller_rx,
                    client_died_error_holder_copy,
                );
                future::finished(())
            });
            Completion::Rx(done_rx)
        } else {
            // Start event loop.
            let tls = self.tls;
            let conf = self.conf;
            let thread_name = conf
                .thread_name
                .clone()
                .unwrap_or_else(|| "http2-client-loop".to_owned())
                .to_string();
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
                        controller_rx,
                        client_died_error_holder_copy,
                    );

                    lp.run(done_rx).expect("run");
                }).expect("spawn");
            Completion::Thread(join_handle)
        };

        Ok(Client {
            join: Some(join),
            controller_tx,
            http_scheme,
            shutdown: shutdown_signal,
            client_died_error_holder,
        })
    }
}

enum Completion {
    Thread(thread::JoinHandle<()>),
    Rx(oneshot::Receiver<()>),
}

/// Asynchronous HTTP/2 client.
///
/// Client connects to the single server address (which must be specified
/// in `ClientBuilder`). When connection fails (because of network error
/// or protocol error) client is reconnected.
pub struct Client {
    controller_tx: UnboundedSender<ControllerCommand>,
    join: Option<Completion>,
    http_scheme: HttpScheme,
    // used only once to send shutdown signal
    shutdown: ShutdownSignal,
    client_died_error_holder: ClientDiedErrorHolder<ClientDiedType>,
}

impl Client {
    /// Create a new client connected to the specified host and port without using TLS.
    pub fn new_plain(host: &str, port: u16, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::new_plain();
        client.conf = conf;
        client.set_addr((host, port))?;
        client.build()
    }

    /// Create a new client connected to the specified host and port using TLS.
    pub fn new_tls<C: TlsConnector>(host: &str, port: u16, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::<C>::new();
        client.conf = conf;
        client.set_addr((host, port))?;
        client.set_tls(host)?;
        client.build()
    }

    /// Create a new client connected to the specified localhost Unix addr.
    #[cfg(unix)]
    pub fn new_plain_unix(addr: &str, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::new_plain();
        client.conf = conf;
        client.set_unix_addr(addr)?;
        client.build()
    }

    /// Create a new client connected to the specified localhost Unix addr using TLS.
    #[cfg(unix)]
    pub fn new_tls_unix<C: TlsConnector>(addr: &str, conf: ClientConf) -> Result<Client> {
        let mut client = ClientBuilder::<C>::new();
        client.conf = conf;
        client.set_unix_addr(addr)?;
        client.build()
    }

    /// Connect to server using plain or TLS protocol depending on `tls` parameter.
    pub fn new_expl<C: TlsConnector>(
        addr: &SocketAddr,
        tls: ClientTlsOption<C>,
        conf: ClientConf,
    ) -> Result<Client> {
        let mut client = ClientBuilder::new();
        client.addr = Some(AnySocketAddr::Inet(addr.clone()));
        client.tls = tls;
        client.conf = conf;
        client.build()
    }

    pub fn start_request_end_stream(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
    ) -> Response {
        Response::new(
            self.start_request_low_level(headers, body, trailers, true)
                .and_then(move |(_sender, response)| response),
        )
    }

    /// Start HTTP/2 `GET` request.
    pub fn start_get(&self, path: &str, authority: &str) -> Response {
        let headers = Headers::from_vec(vec![
            Header::new(":method", "GET"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_end_stream(headers, None, None)
    }

    /// Start HTTP/2 `POST` request.
    pub fn start_post(&self, path: &str, authority: &str, body: Bytes) -> Response {
        let headers = Headers::from_vec(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_end_stream(headers, Some(body), None)
    }

    pub fn start_post_sink(
        &self,
        path: &str,
        authority: &str,
    ) -> HttpFutureSend<(ClientRequest, Response)> {
        let headers = Headers::from_vec(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_low_level(headers, None, None, false)
    }

    /// For tests
    #[doc(hidden)]
    pub fn dump_state(&self) -> HttpFutureSend<ConnStateSnapshot> {
        let (tx, rx) = oneshot::channel();
        // ignore error
        drop(
            self.controller_tx
                .unbounded_send(ControllerCommand::_DumpState(tx)),
        );
        Box::new(rx.map_err(|_| error::Error::Other("conn died")))
    }

    /// Create a future which waits for successful connection.
    pub fn wait_for_connect(&self) -> HttpFutureSend<()> {
        let (tx, rx) = oneshot::channel();
        // ignore error
        drop(
            self.controller_tx
                .unbounded_send(ControllerCommand::WaitForConnect(tx)),
        );
        // TODO: return client death reason
        Box::new(
            rx.map_err(|_| error::Error::Other("conn died"))
                .and_then(|r| r),
        )
    }
}

pub trait ClientInterface {
    /// Start HTTP/2 request.
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
    ) -> HttpFutureSend<(ClientRequest, Response)>;
}

impl ClientInterface for Client {
    // TODO: copy-paste with ClientConn::start_request
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
    ) -> HttpFutureSend<(ClientRequest, Response)> {
        let (resp_tx, resp_rx) = oneshot::channel();

        let start = StartRequestMessage {
            headers,
            body,
            trailers,
            end_stream,
            resp_tx,
        };

        if let Err(_) = self
            .controller_tx
            .unbounded_send(ControllerCommand::StartRequest(start))
        {
            // TODO: named error
            return Box::new(future::err(error::Error::Other("client controller died")));
        }

        let client_error = self.client_died_error_holder.clone();
        let resp_rx = resp_rx.map_err(move |oneshot::Canceled| client_error.error());

        Box::new(resp_rx.flatten())
    }
}

enum ControllerCommand {
    GoAway,
    StartRequest(StartRequestMessage),
    WaitForConnect(oneshot::Sender<Result<()>>),
    _DumpState(oneshot::Sender<ConnStateSnapshot>),
}

struct ControllerState<T: ToClientStream, C: TlsConnector> {
    handle: reactor::Handle,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    // current connection
    conn: Arc<ClientConn>,
    tx: UnboundedSender<ControllerCommand>,
}

impl<T: ToClientStream + 'static + Clone, C: TlsConnector> ControllerState<T, C> {
    fn init_conn(&mut self) {
        let conn = ClientConn::spawn(
            self.handle.clone(),
            Box::new(self.socket_addr.clone()),
            self.tls.clone(),
            self.conf.clone(),
            CallbacksImpl {
                tx: self.tx.clone(),
            },
        );

        self.conn = Arc::new(conn);
    }

    fn iter(mut self, cmd: ControllerCommand) -> ControllerState<T, C> {
        match cmd {
            ControllerCommand::GoAway => {
                self.init_conn();
            }
            ControllerCommand::StartRequest(start) => {
                if let Err(start) = self.conn.start_request_with_resp_sender(start) {
                    self.init_conn();
                    if let Err(start) = self.conn.start_request_with_resp_sender(start) {
                        let err = error::Error::Other("client died and reconnect failed");
                        // ignore error
                        if let Err(_) = start.resp_tx.send(Err(err)) {
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
            ControllerCommand::_DumpState(tx) => {
                self.conn.dump_state_with_resp_sender(tx);
            }
        }
        self
    }

    fn run(self, rx: UnboundedReceiver<ControllerCommand>) -> HttpFuture<()> {
        let rx = rx.map_err(|_| error::Error::Other("channel died"));
        let r = rx.fold(self, |state, cmd| Ok::<_, error::Error>(state.iter(cmd)));
        let r = r.map(|_| ());
        Box::new(r)
    }
}

struct CallbacksImpl {
    tx: UnboundedSender<ControllerCommand>,
}

impl ClientConnCallbacks for CallbacksImpl {
    fn goaway(&self, _stream_id: StreamId, _error_code: u32) {
        drop(self.tx.unbounded_send(ControllerCommand::GoAway));
    }
}

// Event loop entry point
fn spawn_client_event_loop<T: ToClientStream + Send + Clone + 'static, C: TlsConnector>(
    handle: reactor::Handle,
    shutdown_future: ShutdownFuture,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    done_tx: oneshot::Sender<()>,
    controller_tx: UnboundedSender<ControllerCommand>,
    controller_rx: UnboundedReceiver<ControllerCommand>,
    client_died_error_holder: ClientDiedErrorHolder<ClientDiedType>,
) {
    let http_conn = ClientConn::spawn(
        handle.clone(),
        Box::new(socket_addr.clone()),
        tls.clone(),
        conf.clone(),
        CallbacksImpl {
            tx: controller_tx.clone(),
        },
    );

    let init = ControllerState {
        handle: handle.clone(),
        socket_addr: socket_addr.clone(),
        tls: tls,
        conf: conf,
        conn: Arc::new(http_conn),
        tx: controller_tx,
    };

    let controller_future = init.run(controller_rx);

    let shutdown_future = shutdown_future.then(move |_| {
        // Must complete with error,
        // so `join` with this future cancels another future.
        future::failed::<(), _>(Error::Shutdown)
    });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = controller_future.join(shutdown_future);

    let done = done.map(|((), ())| ());

    let done = done.then(|r| {
        // OK to ignore error, because rx might be already dead
        drop(done_tx.send(()));
        r
    });

    let done = client_died_error_holder.wrap_future(done);

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
            }
        };
    }
}
