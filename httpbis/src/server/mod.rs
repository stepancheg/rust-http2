pub mod conf;
pub mod conn;
pub mod handler;
pub mod handler_paths;
pub(crate) mod increase_in_window;
pub mod req;
pub mod resp;
pub(crate) mod stream_handler;
pub mod tls;
pub(crate) mod types;

use futures::future::try_join_all;
use std::collections::HashMap;

use std::net::ToSocketAddrs;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use tls_api;

use futures::channel::oneshot;
use futures::future;
use futures::future::try_join;
use futures::future::FutureExt;
use futures::future::TryFutureExt;

use crate::error::Error;
use crate::result::Result;

use crate::solicit_async::*;

use crate::futures_misc::*;

use tls_api::TlsAcceptor;
use tls_api_stub;

use crate::net::addr::AnySocketAddr;
use crate::net::listen::ToSocketListener;
use crate::net::listen::ToTokioListener;

pub use self::tls::ServerTlsOption;
use crate::assert_types::assert_send_future;
use crate::common::conn::ConnStateSnapshot;
use crate::net::unix::SocketAddrUnix;
use crate::result;
pub use crate::server::conf::ServerConf;
pub use crate::server::conn::ServerConn;
use crate::server::handler::ServerHandler;
use crate::server::handler_paths::ServerHandlerPaths;
use rand::thread_rng;
use rand::Rng;
use std::fmt;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

pub struct ServerBuilder<A: tls_api::TlsAcceptor = tls_api_stub::TlsAcceptor> {
    pub conf: ServerConf,
    pub tls: ServerTlsOption<A>,
    pub addr: Option<AnySocketAddr>,
    /// Event loop to spawn server.
    /// If not specified, builder will create new event loop in a new thread.
    pub event_loop: Option<Handle>,
    /// Event loops used to run incoming connections.
    /// If empty, listener event loop will be used.
    // TODO: test it
    pub conn_event_loops: Vec<Handle>,
    pub service: ServerHandlerPaths,
}

impl ServerBuilder<tls_api_stub::TlsAcceptor> {
    /// New server builder with defaults.
    ///
    /// Port must be set, other properties are optional.
    pub fn new_plain() -> ServerBuilder<tls_api_stub::TlsAcceptor> {
        ServerBuilder::new()
    }
}

#[cfg(unix)]
impl ServerBuilder<tls_api_stub::TlsAcceptor> {
    /// New unix domain socket server with defaults
    ///
    /// Addr must be set, other properties are optional.
    pub fn new_plain_unix() -> ServerBuilder<tls_api_stub::TlsAcceptor> {
        ServerBuilder::<tls_api_stub::TlsAcceptor>::new()
    }
}

impl<A: tls_api::TlsAcceptor> ServerBuilder<A> {
    /// Set port server listens on.
    /// Can be zero to bind on any available port,
    /// which can be later obtained by `Server::local_addr`.
    pub fn set_port(&mut self, port: u16) {
        self.set_addr(("::", port)).expect("set_addr");
    }

    /// Set port server listens on.
    pub fn set_addr<S: ToSocketAddrs>(&mut self, addr: S) -> Result<()> {
        let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(Error::AddrResolvedToEmptyList);
        } else if addrs.len() > 1 {
            return Err(Error::AddrResolvedToMoreThanOneAddr(addrs));
        }
        self.addr = Some(AnySocketAddr::Inet(addrs.into_iter().next().unwrap()));
        Ok(())
    }
}

impl<A: tls_api::TlsAcceptor> ServerBuilder<A> {
    // Set name of unix domain socket
    pub fn set_unix_addr<S: Into<SocketAddrUnix>>(&mut self, addr: S) -> Result<()> {
        self.addr = Some(AnySocketAddr::Unix(addr.into()));
        Ok(())
    }
}

impl<A: tls_api::TlsAcceptor> ServerBuilder<A> {
    /// New server builder with defaults.
    ///
    /// To call this function `ServerBuilder` must be parameterized with TLS acceptor.
    /// If TLS is not needed, `ServerBuilder::new_plain` function can be used.
    ///
    /// Port must be set, other properties are optional.
    pub fn new() -> ServerBuilder<A> {
        ServerBuilder {
            conf: ServerConf::new(),
            tls: ServerTlsOption::Plain,
            addr: None,
            event_loop: None,
            conn_event_loops: Vec::new(),
            service: ServerHandlerPaths::new(),
        }
    }

    pub fn set_tls(&mut self, acceptor: A) {
        self.tls = ServerTlsOption::Tls(Arc::new(acceptor));
    }

    pub fn build(self) -> Result<Server> {
        let (alive_tx, alive_rx) = mpsc::channel();

        let state: Arc<Mutex<ServerState>> = Default::default();

        let state_copy = state.clone();

        let (shutdown_signal, shutdown_future) = shutdown_signal();

        // TODO: why done_tx is unused?
        let (_done_tx, done_rx) = oneshot::channel();

        let listen = match self.addr {
            Some(addr) => addr.listen(&self.conf)?,
            None => return Err(Error::ListenAddrNotSpecified),
        };

        let local_addr = listen.local_addr().unwrap();
        //let local_addr = local_addr.downcast_ref::<T>().expect("downcast socket_addr").clone();

        let join = if let Some(remote) = self.event_loop {
            let tls = self.tls;
            let conf = self.conf;
            let service = self.service;
            let conn_event_loops = self.conn_event_loops;
            let handle = remote.clone();
            remote.spawn(spawn_server_event_loop(
                handle.clone(),
                conn_event_loops,
                state_copy,
                tls,
                listen,
                shutdown_future,
                conf,
                service,
                alive_tx,
            ));
            Completion::Rx(done_rx)
        } else {
            let tls = self.tls;
            let conf = self.conf;
            let service = self.service;
            let conn_event_loops = self.conn_event_loops;
            let join_handle = thread::Builder::new()
                .name(
                    conf.thread_name
                        .clone()
                        .unwrap_or_else(|| "http2-server-loop".to_owned())
                        .to_string(),
                )
                .spawn(move || {
                    let lp = Runtime::new().expect("http2server");
                    lp.block_on(
                        spawn_server_event_loop(
                            lp.handle().clone(),
                            conn_event_loops,
                            state_copy,
                            tls,
                            listen,
                            shutdown_future,
                            conf,
                            service,
                            alive_tx,
                        )
                        .map(|_| ()),
                    );
                })?;
            Completion::Thread(join_handle)
        };

        Ok(Server {
            state: state,
            shutdown: shutdown_signal,
            local_addr: local_addr,
            join: Some(join),
            alive_rx: alive_rx,
        })
    }
}

enum Completion {
    Thread(thread::JoinHandle<()>),
    Rx(oneshot::Receiver<()>),
}

pub struct Server {
    state: Arc<Mutex<ServerState>>,
    local_addr: AnySocketAddr,
    shutdown: ShutdownSignal,
    alive_rx: mpsc::Receiver<()>,
    join: Option<Completion>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Server")
            .field("local_addr", &self.local_addr)
            .finish()
    }
}

#[derive(Default)]
struct ServerState {
    last_conn_id: u64,
    conns: HashMap<u64, ServerConn>,
}

impl ServerState {
    fn snapshot(&self) -> HttpFutureSend<ServerStateSnapshot> {
        let futures: Vec<_> = self
            .conns
            .iter()
            .map(|(&id, conn)| {
                assert_send_future::<result::Result<_>, _>(
                    conn.dump_state().map_ok(move |state| (id, state)),
                )
            })
            .collect();

        let j = try_join_all(futures);
        let j = assert_send_future::<result::Result<_>, _>(j);

        Box::pin(j.map_ok(|states| ServerStateSnapshot {
            conns: states.into_iter().collect(),
        }))
    }
}

#[derive(Debug)]
pub struct ServerStateSnapshot {
    pub conns: HashMap<u64, ConnStateSnapshot>,
}

impl ServerStateSnapshot {
    pub fn single_conn(&self) -> (u64, &ConnStateSnapshot) {
        let mut iter = self.conns.iter();
        let (&id, conn) = iter.next().expect("no conns");
        assert!(iter.next().is_none(), "more than one conn");
        (id, conn)
    }
}

fn spawn_server_event_loop<S, A>(
    handle: Handle,
    mut conn_handles: Vec<Handle>,
    state: Arc<Mutex<ServerState>>,
    tls: ServerTlsOption<A>,
    listen: Box<dyn ToTokioListener + Send>,
    shutdown_future: ShutdownFuture,
    conf: ServerConf,
    service: S,
    _alive_tx: mpsc::Sender<()>,
) -> oneshot::Receiver<()>
where
    S: ServerHandler,
    A: TlsAcceptor,
{
    let service = Arc::new(service);

    let mut tokio_listener = listen.into_tokio_listener(&handle);

    if conn_handles.is_empty() {
        conn_handles.push(handle.clone());
    }

    let loop_run = async move {
        if false {
            // type hint
            return Ok(());
        }

        loop {
            let (socket, peer_addr) = tokio_listener.as_mut().accept().await?;

            info!("accepted connection from {}", peer_addr);

            if socket.is_tcp() {
                let no_delay = conf.no_delay.unwrap_or(true);
                socket
                    .set_tcp_nodelay(no_delay)
                    .expect("failed to set TCP_NODELAY");
            }

            // TODO: implement smarter selection
            let handle = conn_handles[thread_rng().gen_range(0, conn_handles.len())].clone();
            let handle_clone = handle.clone();
            let state_clone = state.clone();
            handle.spawn({
                let (conn, future) = ServerConn::new(
                    &handle_clone,
                    socket,
                    peer_addr,
                    tls.clone(),
                    conf.clone(),
                    service.clone(),
                );

                let conn_id = {
                    let mut g = state_clone.lock().expect("lock");
                    g.last_conn_id += 1;
                    let conn_id = g.last_conn_id;
                    let prev = g.conns.insert(conn_id, conn);
                    assert!(prev.is_none());
                    conn_id
                };

                let future = assert_send_future::<result::Result<()>, _>(future);

                FutureExt::then(future, move |r| {
                    let mut g = state_clone.lock().expect("lock");
                    let removed = g.conns.remove(&conn_id);
                    assert!(removed.is_some());
                    future::ready(r)
                })
                .map_err(|e| {
                    warn!("connection end: {:?}", e);
                    ()
                })
            });
        }
    };

    let (done_tx, done_rx) = oneshot::channel();

    let shutdown_future = shutdown_future.then(move |_| {
        // Must complete with error,
        // so `join` with this future cancels another future.
        future::err::<(), _>(Error::Shutdown)
    });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = try_join(loop_run, shutdown_future);

    let done = assert_send_future::<result::Result<_>, _>(done);

    let done = done.then(|_| {
        drop(done_tx.send(()));
        future::ready(())
    });

    handle.spawn(done);

    done_rx
}

impl Server {
    pub fn local_addr(&self) -> &AnySocketAddr {
        &self.local_addr
    }

    pub fn is_alive(&self) -> bool {
        self.alive_rx.try_recv() != Err(mpsc::TryRecvError::Disconnected)
    }

    // for tests
    pub fn dump_state(&self) -> HttpFutureSend<ServerStateSnapshot> {
        let g = self.state.lock().expect("lock");
        g.snapshot()
    }
}

// We shutdown the server in the destructor.
impl Drop for Server {
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

        self.local_addr.cleanup();
    }
}
