use std::fmt;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use futures::channel::oneshot;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use tls_api::TlsConnector;
use tls_api::TlsConnectorBuilder;
use tls_api_stub;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

use crate::client::conf::ClientConf;
use crate::client::conn::ClientConn;
use crate::client::conn::ClientConnCallbacks;
use crate::client::conn::StartRequestMessage;
use crate::client::req::ClientRequest;
use crate::client::resp::ClientResponse;
use crate::client::stream_handler::ClientStreamCreatedHandler;
pub use crate::client::tls::ClientTlsOption;
use crate::common::conn::ConnStateSnapshot;
use crate::death::channel::death_aware_channel;
use crate::death::channel::DeathAwareReceiver;
use crate::death::channel::DeathAwareSender;
use crate::death::channel::ErrorAwareDrop;
use crate::death::error_holder::ClientDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::death::oneshot::death_aware_oneshot;
use crate::death::oneshot::DeathAwareOneshotSender;
use crate::error;
use crate::error::Error;
use crate::futures_misc::*;
use crate::net::addr::AnySocketAddr;
use crate::net::connect::ToClientStream;
use crate::net::unix::SocketAddrUnix;
use crate::result;
use crate::result::Result;
use crate::solicit::header::*;
use crate::solicit::stream_id::StreamId;
use crate::solicit::HttpScheme;
use crate::solicit_async::*;
use crate::ClientResponseFuture;

pub(crate) mod conf;
pub(crate) mod conn;
pub(crate) mod increase_in_window;
pub(crate) mod req;
pub(crate) mod resp;
pub mod resp_future;
pub(crate) mod stream_handler;
pub(crate) mod tls;
pub(crate) mod types;

/// Builder for HTTP/2 client.
///
/// Client parameters can be specified only during construction,
/// and later client cannot be reconfigured.
pub struct ClientBuilder<C: TlsConnector = tls_api_stub::TlsConnector> {
    pub event_loop: Option<Handle>,
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
            return Err(Error::AddrResolvedToEmptyList);
        } else if addrs.len() > 1 {
            // TODO: allow multiple addresses
            return Err(Error::AddrResolvedToMoreThanOneAddr(addrs));
        }
        self.addr = Some(AnySocketAddr::Inet(addrs.into_iter().next().unwrap()));
        Ok(())
    }
}

impl<C: TlsConnector> ClientBuilder<C> {
    /// Set the addr client connects to.
    pub fn set_unix_addr<A: Into<SocketAddrUnix>>(&mut self, addr: A) -> Result<()> {
        self.addr = Some(AnySocketAddr::Unix(addr.into()));
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

        if C::SUPPORTS_ALPN {
            // TODO: check negotiated protocol after connect
            tls_connector.set_alpn_protocols(&[b"h2"])?;
        }

        let tls_connector = tls_connector.build()?;

        let tls_connector = Arc::new(tls_connector);
        self.tls = ClientTlsOption::Tls(host.to_owned(), tls_connector);
        Ok(())
    }

    pub fn build(self) -> Result<Client> {
        let client_died_error_holder = SomethingDiedErrorHolder::new();

        let addr = self.addr.expect("addr is not specified");
        let addr_copy = addr.clone();

        let http_scheme = self.tls.http_scheme();

        // Create a channel to receive shutdown signal.
        let (shutdown_signal, shutdown_future) = shutdown_signal();

        let (controller_tx, controller_rx) = death_aware_channel(client_died_error_holder.clone());

        let (done_tx, done_rx) = oneshot::channel();

        let client_died_error_holder_copy = client_died_error_holder.clone();

        let join = if let Some(remote) = self.event_loop {
            let tls = self.tls;
            let conf = self.conf;
            let controller_tx = controller_tx.clone();
            let handle = remote.clone();
            remote.spawn(future::lazy(move |_cx| {
                spawn_client_event_loop(
                    handle,
                    shutdown_future,
                    addr_copy,
                    tls,
                    conf,
                    done_tx,
                    controller_tx,
                    controller_rx,
                    client_died_error_holder_copy,
                )
            }));
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
                    let lp: Runtime = Runtime::new().expect("Core::new");

                    spawn_client_event_loop(
                        lp.handle().clone(),
                        shutdown_future,
                        addr_copy,
                        tls,
                        conf,
                        done_tx,
                        controller_tx,
                        controller_rx,
                        client_died_error_holder_copy,
                    );

                    lp.block_on(done_rx).expect("run");
                })
                .expect("spawn");
            Completion::Thread(join_handle)
        };

        Ok(Client {
            join: Some(join),
            controller_tx,
            http_scheme,
            shutdown: shutdown_signal,
            client_died_error_holder,
            addr,
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
    controller_tx: DeathAwareSender<ControllerCommand>,
    join: Option<Completion>,
    http_scheme: HttpScheme,
    // used only once to send shutdown signal
    shutdown: ShutdownSignal,
    client_died_error_holder: SomethingDiedErrorHolder<ClientDiedType>,
    addr: AnySocketAddr,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("addr", &self.addr)
            .field("http_scheme", &self.http_scheme)
            .finish()
    }
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

    pub fn start_request(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
    ) -> HttpFutureSend<(ClientRequest, ClientResponseFuture)> {
        let (tx, rx) = oneshot::channel();

        struct Impl {
            tx: oneshot::Sender<crate::Result<(ClientRequest, ClientResponseFuture)>>,
        }

        impl ClientStreamCreatedHandler for Impl {
            fn request_created(
                self: Box<Self>,
                req: ClientRequest,
                resp: ClientResponse,
            ) -> result::Result<()> {
                if let Err(_) = self.tx.send(Ok((req, resp.into_stream()))) {
                    return Err(error::Error::CallerDied);
                }

                Ok(())
            }

            fn error(self: Box<Self>, error: crate::Error) {
                let _ = self.tx.send(Err(error));
            }
        }

        self.start_request_low_level(headers, body, trailers, end_stream, Box::new(Impl { tx }));

        let client_error = self.client_died_error_holder.clone();
        let resp_rx = rx.then(move |r| match r {
            Ok(Ok(r)) => future::ok(r),
            Ok(Err(e)) => future::err(e),
            Err(oneshot::Canceled) => future::err(client_error.error()),
        });

        Box::pin(resp_rx)
    }

    pub fn start_request_end_stream(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
    ) -> ClientResponseFuture {
        ClientResponseFuture::new(
            self.start_request(headers, body, trailers, true)
                .and_then(move |(_sender, response)| response),
        )
    }

    /// Start HTTP/2 `GET` request.
    pub fn start_get(&self, path: &str, authority: &str) -> ClientResponseFuture {
        let headers = Headers::from_vec(vec![
            Header::new(":method", "GET"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request_end_stream(headers, None, None)
    }

    /// Start HTTP/2 `POST` request.
    pub fn start_post(&self, path: &str, authority: &str, body: Bytes) -> ClientResponseFuture {
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
    ) -> HttpFutureSend<(ClientRequest, ClientResponseFuture)> {
        let headers = Headers::from_vec(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path.to_owned()),
            Header::new(":authority", authority.to_owned()),
            Header::new(":scheme", self.http_scheme.as_bytes()),
        ]);
        self.start_request(headers, None, None, false)
    }

    /// For tests
    #[doc(hidden)]
    pub fn dump_state(&self) -> HttpFutureSend<ConnStateSnapshot> {
        let (tx, rx) = death_aware_oneshot(self.client_died_error_holder.clone());
        // ignore error
        drop(
            self.controller_tx
                .unbounded_send(ControllerCommand::DumpState(tx)),
        );
        Box::pin(rx)
    }

    /// Create a future which waits for successful connection.
    pub fn wait_for_connect(&self) -> HttpFutureSend<()> {
        let (tx, rx) = oneshot::channel();
        // ignore error
        drop(
            self.controller_tx
                .unbounded_send(ControllerCommand::WaitForConnect(tx)),
        );
        // TODO: no need to clone
        let client_died_error_holder = self.client_died_error_holder.clone();
        Box::pin(
            rx.map_err(move |_| client_died_error_holder.error())
                .and_then(|r| future::ready(r)),
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
        stream_handler: Box<dyn ClientStreamCreatedHandler>,
    );
}

impl ClientInterface for Client {
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
        stream_handler: Box<dyn ClientStreamCreatedHandler>,
    ) {
        let start = StartRequestMessage {
            headers,
            body,
            trailers,
            end_stream,
            stream_handler,
        };

        self.controller_tx
            .unbounded_send_no_result(ControllerCommand::StartRequest(start))
    }
}

enum ControllerCommand {
    GoAway,
    StartRequest(StartRequestMessage),
    WaitForConnect(oneshot::Sender<Result<()>>),
    DumpState(DeathAwareOneshotSender<ConnStateSnapshot, ClientDiedType>),
}

impl ErrorAwareDrop for ControllerCommand {
    type DiedType = ClientDiedType;

    fn drop_with_error(self, error: Error) {
        match self {
            ControllerCommand::GoAway => {}
            ControllerCommand::StartRequest(start) => start.stream_handler.error(error),
            ControllerCommand::WaitForConnect(wait) => {
                let _ = wait.send(Err(error));
            }
            ControllerCommand::DumpState(_) => {}
        }
    }
}

struct ControllerState<T: ToClientStream, C: TlsConnector> {
    handle: Handle,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    // current connection
    conn: Arc<ClientConn>,
    tx: DeathAwareSender<ControllerCommand>,
}

impl<T: ToClientStream + 'static + Clone, C: TlsConnector> ControllerState<T, C> {
    fn init_conn(&mut self) {
        let conn = ClientConn::spawn(
            self.handle.clone(),
            Box::pin(self.socket_addr.clone()),
            self.tls.clone(),
            self.conf.clone(),
            CallbacksImpl {
                tx: self.tx.clone(),
            },
        );

        self.conn = Arc::new(conn);
    }

    fn iter(&mut self, cmd: ControllerCommand) {
        match cmd {
            ControllerCommand::GoAway => {
                self.init_conn();
            }
            ControllerCommand::StartRequest(start) => {
                if let Err((start, _)) = self.conn.start_request_with_resp_sender(start) {
                    self.init_conn();
                    if let Err((start, e)) = self.conn.start_request_with_resp_sender(start) {
                        warn!("client died and reconnect failed");
                        start.stream_handler.error(e);
                    }
                }
            }
            ControllerCommand::WaitForConnect(tx) => {
                if let Err(tx) = self.conn.wait_for_connect_with_resp_sender(tx) {
                    self.init_conn();
                    if let Err(tx) = self.conn.wait_for_connect_with_resp_sender(tx) {
                        // TODO: reason
                        let err = error::Error::ClientDiedAndReconnectFailed;
                        // ignore error
                        drop(tx.send(Err(err)));
                    }
                }
            }
            ControllerCommand::DumpState(tx) => {
                let (conn_tx, conn_rx) =
                    death_aware_oneshot(self.conn.conn_died_error_holder.clone());
                self.conn.dump_state_with_resp_sender(conn_tx);
                self.handle.spawn(async move {
                    match conn_rx.await {
                        Ok(snapshot) => tx.send(snapshot),
                        Err(_) => {
                            // TODO: pass it
                        }
                    }
                });
            }
        }
    }

    async fn run(mut self, mut rx: DeathAwareReceiver<ControllerCommand>) {
        loop {
            match rx.next().await {
                None => return,
                Some(cmd) => self.iter(cmd),
            }
        }
    }
}

struct CallbacksImpl {
    tx: DeathAwareSender<ControllerCommand>,
}

impl ClientConnCallbacks for CallbacksImpl {
    fn goaway(&self, _stream_id: StreamId, _error_code: u32) {
        drop(self.tx.unbounded_send(ControllerCommand::GoAway));
    }
}

// Event loop entry point
fn spawn_client_event_loop<T: ToClientStream + Send + Clone + 'static, C: TlsConnector>(
    handle: Handle,
    shutdown_future: ShutdownFuture,
    socket_addr: T,
    tls: ClientTlsOption<C>,
    conf: ClientConf,
    done_tx: oneshot::Sender<()>,
    controller_tx: DeathAwareSender<ControllerCommand>,
    controller_rx: DeathAwareReceiver<ControllerCommand>,
    client_died_error_holder: SomethingDiedErrorHolder<ClientDiedType>,
) {
    let http_conn = ClientConn::spawn(
        handle.clone(),
        Box::pin(socket_addr.clone()),
        tls.clone(),
        conf.clone(),
        CallbacksImpl {
            tx: controller_tx.clone(),
        },
    );

    let init = ControllerState {
        handle: handle.clone(),
        socket_addr: socket_addr.clone(),
        tls,
        conf,
        conn: Arc::new(http_conn),
        tx: controller_tx,
    };

    let controller_future = init.run(controller_rx);

    let shutdown_future = shutdown_future.then(move |_| {
        info!("shutdown requested");
        // Must complete with error,
        // so `join` with this future cancels another future.
        future::err::<(), _>(Error::Shutdown)
    });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = future::try_join(controller_future.map(Ok), shutdown_future);

    let done = done.map_ok(|((), ())| ());

    let done = done.then(|r| {
        // OK to ignore error, because rx might be already dead
        drop(done_tx.send(()));
        future::ready(r)
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
