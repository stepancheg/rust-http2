pub mod conf;
pub mod conn;
pub mod handler;
pub mod handler_paths;
pub mod req;
pub mod resp;
pub mod tls;
pub(crate) mod types;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use tls_api;

use tokio_core::reactor;

use futures::future;
use futures::future::join_all;
use futures::future::Future;
use futures::stream;
use futures::stream::Stream;
use futures::sync::oneshot;

use error::Error;
use result::Result;

use solicit_async::*;

use futures_misc::*;

use tls_api::TlsAcceptor;
use tls_api_stub;

use socket::AnySocketAddr;
use socket::ToSocketListener;
use socket::ToTokioListener;

pub use self::tls::ServerTlsOption;
use common::conn::ConnStateSnapshot;
use rand::thread_rng;
use rand::Rng;
pub use server::conf::ServerConf;
pub use server::conn::ServerConn;
use server::handler::ServerHandler;
use server::handler_paths::ServerHandlerPaths;

pub struct ServerBuilder<A: tls_api::TlsAcceptor = tls_api_stub::TlsAcceptor> {
    pub conf: ServerConf,
    pub tls: ServerTlsOption<A>,
    pub addr: Option<AnySocketAddr>,
    /// Event loop to spawn server.
    /// If not specified, builder will create new event loop in a new thread.
    pub event_loop: Option<reactor::Remote>,
    /// Event loops used to run incoming connections.
    /// If empty, listener event loop will be used.
    // TODO: test it
    pub conn_event_loops: Vec<reactor::Remote>,
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
            return Err(Error::Other("addr is resolved to empty list"));
        } else if addrs.len() > 1 {
            return Err(Error::Other("addr is resolved to more than one addr"));
        }
        self.addr = Some(AnySocketAddr::Inet(addrs.into_iter().next().unwrap()));
        Ok(())
    }
}

#[cfg(unix)]
impl<A: tls_api::TlsAcceptor> ServerBuilder<A> {
    // Set name of unix domain socket
    pub fn set_unix_addr(&mut self, addr: String) -> Result<()> {
        self.addr = Some(AnySocketAddr::Unix(addr));
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

        // TODO: return result
        let listen = self
            .addr
            .expect("listen addr not specified")
            .to_listener(&self.conf);

        let local_addr = listen.local_addr().unwrap();
        //let local_addr = local_addr.downcast_ref::<T>().expect("downcast socket_addr").clone();

        let join = if let Some(remote) = self.event_loop {
            let tls = self.tls;
            let conf = self.conf;
            let service = self.service;
            let conn_event_loops = self.conn_event_loops;
            remote.spawn(move |handle| {
                drop(spawn_server_event_loop(
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
                future::finished(())
            });
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
                ).spawn(move || {
                    let mut lp = reactor::Core::new().expect("http2server");
                    let done_rx = spawn_server_event_loop(
                        lp.handle(),
                        conn_event_loops,
                        state_copy,
                        tls,
                        listen,
                        shutdown_future,
                        conf,
                        service,
                        alive_tx,
                    );
                    drop(lp.run(done_rx));
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
            .map(|(&id, conn)| conn.dump_state().map(move |state| (id, state)))
            .collect();

        Box::new(join_all(futures).map(|states| ServerStateSnapshot {
            conns: states.into_iter().collect(),
        }))
    }
}

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
    handle: reactor::Handle,
    mut conn_handles: Vec<reactor::Remote>,
    state: Arc<Mutex<ServerState>>,
    tls: ServerTlsOption<A>,
    listen: Box<ToTokioListener + Send>,
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

    let tokio_listener = listen.to_tokio_listener(&handle);

    if conn_handles.is_empty() {
        conn_handles.push(handle.remote().clone());
    }

    let stuff = stream::repeat((conn_handles, service, state, tls, conf));

    let loop_run = tokio_listener
        .incoming()
        .map_err(Error::from)
        .zip(stuff)
        .for_each(
            move |((socket, peer_addr), (conn_handles, service, state, tls, conf))| {
                if socket.is_tcp() {
                    info!(
                        "accepted connection from {}",
                        peer_addr.downcast_ref::<SocketAddr>().unwrap()
                    );

                    let no_delay = conf.no_delay.unwrap_or(true);
                    socket
                        .set_nodelay(no_delay)
                        .expect("failed to set TCP_NODELAY");
                }

                // TODO: implement smarter selection
                let handle = conn_handles[thread_rng().gen_range(0, conn_handles.len())].clone();
                handle.spawn(move |handle| {
                    let (conn, future) = ServerConn::new(&handle, socket, tls, conf, service);

                    let conn_id = {
                        let mut g = state.lock().expect("lock");
                        g.last_conn_id += 1;
                        let conn_id = g.last_conn_id;
                        let prev = g.conns.insert(conn_id, conn);
                        assert!(prev.is_none());
                        conn_id
                    };

                    future
                        .then(move |r| {
                            let mut g = state.lock().expect("lock");
                            let removed = g.conns.remove(&conn_id);
                            assert!(removed.is_some());
                            r
                        }).map_err(|e| {
                            warn!("connection end: {:?}", e);
                            ()
                        })
                });
                Ok(())
            },
        );

    let (done_tx, done_rx) = oneshot::channel();

    let shutdown_future = shutdown_future.then(move |_| {
        // Must complete with error,
        // so `join` with this future cancels another future.
        future::failed::<(), _>(Error::Shutdown)
    });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = loop_run.join(shutdown_future);

    let done = done.then(|_| {
        drop(done_tx.send(()));
        Ok(())
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
