use std::thread;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::io;

use tls_api;

use tokio_core::reactor;
use tokio_core::net::TcpListener;

use futures::future;
use futures::future::Future;
use futures::future::join_all;
use futures::stream;
use futures::stream::Stream;

use futures_cpupool;
use futures_cpupool::CpuPool;

use exec::CpuPoolOption;

use error::Error;
use result::Result;

use solicit_async::*;

use futures_misc::*;

use net2;

use tls_api::TlsAcceptor;
use tls_api_stub;

use super::server_conn::*;
use super::common::*;

use service::Service;
use service_paths::ServicePaths;

use server_conf::*;

pub use server_tls::ServerTlsOption;


struct LoopToServer {
    shutdown: ShutdownSignal,
    local_addr: SocketAddr,
}



pub struct ServerBuilder<A : tls_api::TlsAcceptor = tls_api_stub::TlsAcceptor> {
    pub conf: ServerConf,
    pub cpu_pool: CpuPoolOption,
    pub tls: ServerTlsOption<A>,
    pub addr: Option<SocketAddr>,
    pub service: ServicePaths,
}

impl ServerBuilder<tls_api_stub::TlsAcceptor> {
    /// New server builder with defaults.
    ///
    /// Port must be set, other properties are optional.
    pub fn new_plain() -> ServerBuilder<tls_api_stub::TlsAcceptor> {
        ServerBuilder::new()
    }
}

impl<A : tls_api::TlsAcceptor> ServerBuilder<A> {
    /// New server builder with defaults.
    ///
    /// To call this function `ServerBuilder` must be parameterized with TLS acceptor.
    /// If TLS is not needed, `ServerBuilder::new_plain` function can be used.
    ///
    /// Port must be set, other properties are optional.
    pub fn new() -> ServerBuilder<A> {
        ServerBuilder {
            conf: ServerConf::new(),
            cpu_pool: CpuPoolOption::SingleThread,
            tls: ServerTlsOption::Plain,
            addr: None,
            service: ServicePaths::new(),
        }
    }

    /// Create a CPU pool, and use it in HTTP server
    pub fn set_cpu_pool_threads(&mut self, threads: usize) {
        let cpu_pool = futures_cpupool::Builder::new()
            .pool_size(threads)
            .name_prefix("httpbis-server-")
            .create();
        self.cpu_pool = CpuPoolOption::CpuPool(cpu_pool);
    }

    pub fn set_tls(&mut self, acceptor: A) {
        self.tls = ServerTlsOption::Tls(Arc::new(acceptor));
    }

    /// Set port server listens on.
    /// Can be zero to bind on any available port,
    /// which can be later obtained by `Server::local_addr`.
    pub fn set_port(&mut self, port: u16) {
        self.set_addr(("::", port)).expect("set_addr");
    }

    /// Set port server listens on.
    pub fn set_addr<S : ToSocketAddrs>(&mut self, addr: S) -> Result<()> {
        let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(Error::Other("addr is resolved to empty list"));
        } else if addrs.len() > 1 {
            return Err(Error::Other("addr is resolved to more than one addr"));
        }
        self.addr = Some(addrs.into_iter().next().unwrap());
        Ok(())
    }

    pub fn build(self) -> Result<Server> {
        let addr = self.addr.expect("listen addr is unset");
        Server::new(addr, self.tls, self.cpu_pool, self.conf, self.service)
    }
}

pub struct Server {
    state: Arc<Mutex<ServerState>>,
    loop_to_server: LoopToServer,
    alive_rx: mpsc::Receiver<()>,
    thread_join_handle: Option<thread::JoinHandle<()>>,
}

#[derive(Default)]
struct ServerState {
    last_conn_id: u64,
    conns: HashMap<u64, ServerConnection>,
}

impl ServerState {
    fn snapshot(&self) -> HttpFutureSend<ServerStateSnapshot> {
        let futures: Vec<_> = self.conns.iter()
            .map(|(&id, conn)| conn.dump_state().map(move |state| (id, state)))
            .collect();

        Box::new(join_all(futures)
            .map(|states| ServerStateSnapshot {
                conns: states.into_iter().collect(),
            }))
    }
}

pub struct ServerStateSnapshot {
    pub conns: HashMap<u64, ConnectionStateSnapshot>,
}

impl ServerStateSnapshot {
    pub fn single_conn(&self) -> (u64, &ConnectionStateSnapshot) {
        let mut iter = self.conns.iter();
        let (&id, conn) = iter.next().expect("no conns");
        assert!(iter.next().is_none(), "more than one conn");
        (id, conn)
    }
}

#[cfg(unix)]
fn configure_tcp(tcp: &net2::TcpBuilder, conf: &ServerConf) -> io::Result<()> {
    use net2::unix::UnixTcpBuilderExt;
    if let Some(reuse_port) = conf.reuse_port {
        tcp.reuse_port(reuse_port)?;
    }
    Ok(())
}

#[cfg(windows)]
fn configure_tcp(_tcp: &net2::TcpBuilder, conf: &ServerConf) -> io::Result<()> {
    Ok(())
}

fn listener(
    addr: &SocketAddr,
    handle: &reactor::Handle,
    conf: &ServerConf)
        -> io::Result<TcpListener>
{
    let listener = match *addr {
        SocketAddr::V4(_) => net2::TcpBuilder::new_v4()?,
        SocketAddr::V6(_) => net2::TcpBuilder::new_v6()?,
    };
    configure_tcp(&listener, conf)?;
    listener.reuse_address(true)?;
    listener.bind(addr)?;
    let backlog = conf.backlog.unwrap_or(1024);
    let listener = listener.listen(backlog)?;
    TcpListener::from_listener(listener, addr, handle)
}

fn run_server_event_loop<S, A>(
    listen_addr: SocketAddr,
    state: Arc<Mutex<ServerState>>,
    tls: ServerTlsOption<A>,
    exec: CpuPoolOption,
    conf: ServerConf,
    service: S,
    send_to_back: mpsc::Sender<LoopToServer>,
    _alive_tx: mpsc::Sender<()>)
        where S : Service, A : TlsAcceptor,
{
    let service = Arc::new(service);

    let mut lp = reactor::Core::new().expect("http2server");

    let (shutdown_signal, shutdown_future) = shutdown_signal();

    let listen = listener(&listen_addr, &lp.handle(), &conf).unwrap();

    let stuff = stream::repeat((lp.handle(), service, state, tls, conf));

    let local_addr = listen.local_addr().unwrap();
    send_to_back
        .send(LoopToServer { shutdown: shutdown_signal, local_addr: local_addr })
        .expect("send back");

    let loop_run = listen.incoming().map_err(Error::from).zip(stuff)
        .for_each(move |((socket, peer_addr), (loop_handle, service, state, tls, conf))| {
            info!("accepted connection from {}", peer_addr);

            let no_delay = conf.no_delay.unwrap_or(true);
            socket.set_nodelay(no_delay).expect("failed to set TCP_NODELAY");

            let (conn, future) = ServerConnection::new(
                &loop_handle, socket, tls, exec.clone(), conf, service);

            let conn_id = {
                let mut g = state.lock().expect("lock");
                g.last_conn_id += 1;
                let conn_id = g.last_conn_id;
                let prev = g.conns.insert(conn_id, conn);
                assert!(prev.is_none());
                conn_id
            };

            loop_handle.spawn(future
                .then(move |r| {
                    let mut g = state.lock().expect("lock");
                    let removed = g.conns.remove(&conn_id);
                    assert!(removed.is_some());
                    r
                })
                .map_err(|e| { warn!("connection end: {:?}", e); () }));
            Ok(())
        });

    let shutdown_future = shutdown_future
        .then(move |_| {
            // Must complete with error,
            // so `join` with this future cancels another future.
            future::failed::<(), _>(Error::Shutdown)
        });

    // Wait for either completion of connection (i. e. error)
    // or shutdown signal.
    let done = loop_run.join(shutdown_future);

    // TODO: do not ignore error
    lp.run(done).ok();
}

impl Server {
    pub fn new_plain_single_thread<T, S>(addr: T, conf: ServerConf, service: S)
        -> Result<Server>
        where S : Service, T : ToSocketAddrs
    {
        let no_tls: ServerTlsOption<tls_api_stub::TlsAcceptor> = ServerTlsOption::Plain;
        Server::new(addr, no_tls, CpuPoolOption::SingleThread, conf, service)
    }

    pub fn new_plain_cpu_pool<T, S>(addr: T, cpu_pool: CpuPool, conf: ServerConf, service: S)
        -> Result<Server>
        where S : Service, T : ToSocketAddrs
    {
        let no_tls: ServerTlsOption<tls_api_stub::TlsAcceptor> = ServerTlsOption::Plain;
        Server::new(addr, no_tls, CpuPoolOption::CpuPool(cpu_pool), conf, service)
    }

    pub fn new_tls_single_thread<T, S, A>(addr: T, tls: A, conf: ServerConf, service: S)
        -> Result<Server>
        where S : Service, T : ToSocketAddrs, A : TlsAcceptor
    {
        let tls = ServerTlsOption::Tls(Arc::new(tls));
        Server::new(addr, tls, CpuPoolOption::SingleThread, conf, service)
    }

    pub fn new_tls_cpu_pool<T, S, A>(addr: T, tls: A, cpu_pool: CpuPool, conf: ServerConf, service: S)
        -> Result<Server>
        where S : Service, T : ToSocketAddrs, A : TlsAcceptor
    {
        let tls = ServerTlsOption::Tls(Arc::new(tls));
        Server::new(addr, tls, CpuPoolOption::CpuPool(cpu_pool), conf, service)
    }

    pub fn new<T, S, A>(addr: T, tls: ServerTlsOption<A>, exec: CpuPoolOption, conf: ServerConf, service: S)
        -> Result<Server>
        where S : Service, T : ToSocketAddrs, A : TlsAcceptor
    {
        let listen_addr = addr.to_socket_addrs()?.next().unwrap();

        let (get_from_loop_tx, get_from_loop_rx) = mpsc::channel();
        let (alive_tx, alive_rx) = mpsc::channel();

        let state: Arc<Mutex<ServerState>> = Default::default();

        let state_copy = state.clone();

        let join_handle = thread::Builder::new()
            .name(conf.thread_name.clone().unwrap_or_else(|| "http2-server-loop".to_owned()).to_string())
            .spawn(move || {
                run_server_event_loop(
                    listen_addr,
                    state_copy,
                    tls,
                    exec,
                    conf, service,
                    get_from_loop_tx,
                    alive_tx);
            })?;

        let loop_to_server = get_from_loop_rx.recv()
            .map_err(|_| Error::Other("failed to recv from event loop"))?;

        Ok(Server {
            state: state,
            loop_to_server: loop_to_server,
            thread_join_handle: Some(join_handle),
            alive_rx: alive_rx,
        })
    }

    pub fn local_addr(&self) -> &SocketAddr {
        &self.loop_to_server.local_addr
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
        self.loop_to_server.shutdown.shutdown();

        // do not ignore errors of take
        // ignore errors of join, it means that server event loop crashed
        drop(self.thread_join_handle.take().unwrap().join());
    }
}

