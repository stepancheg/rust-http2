use std::io;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::oneshot;

use tokio_core;
use tokio_core::reactor;

use httpbis;
use httpbis::for_test::*;
use httpbis::*;

use super::BIND_HOST;

/// Single connection HTTP/server.
/// Accepts only one connection.
pub struct ServerOneConn {
    from_loop: FromLoop,
    join_handle: Option<thread::JoinHandle<()>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    conn: Arc<Mutex<Option<ServerConn>>>,
}

struct FromLoop {
    port: u16,
}

impl ServerOneConn {
    pub fn new_fn<S>(port: u16, service: S) -> Self
    where
        S: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> httpbis::Result<()>
            + Send
            + Sync
            + 'static,
    {
        ServerOneConn::new_fn_impl(port, service)
    }

    #[allow(dead_code)]
    fn new_fn_impl<S>(port: u16, service: S) -> Self
    where
        S: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> httpbis::Result<()>
            + Send
            + Sync
            + 'static,
    {
        let (from_loop_tx, from_loop_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let conn: Arc<Mutex<Option<ServerConn>>> = Default::default();

        let conn_for_thread = conn.clone();

        let join_handle = thread::Builder::new()
            .name("server_one_conn".to_owned())
            .spawn(move || {
                let mut lp = reactor::Core::new().unwrap();

                let listener = tokio_core::net::TcpListener::bind(
                    &(BIND_HOST, port).to_socket_addrs().unwrap().next().unwrap(),
                    &lp.handle(),
                ).unwrap();

                let actual_port = listener.local_addr().unwrap().port();
                from_loop_tx
                    .send(FromLoop { port: actual_port })
                    .ok()
                    .unwrap();

                let handle = lp.handle();

                let future = listener
                    .incoming()
                    .into_future()
                    .map_err(|_| {
                        httpbis::Error::from(io::Error::new(io::ErrorKind::Other, "something"))
                    }).and_then(move |(conn, listener)| {
                        // incoming stream is endless
                        let (conn, _) = conn.unwrap();

                        // close listening port
                        drop(listener);

                        let (conn, future) = ServerConn::new_plain_single_thread_fn(
                            &handle,
                            conn,
                            Default::default(),
                            service,
                        );
                        *conn_for_thread.lock().unwrap() = Some(conn);
                        future
                    });

                let shutdown_rx = shutdown_rx.then(|_| future::finished::<_, ()>(()));
                let future = future.then(|_| future::finished::<_, ()>(()));

                lp.run(shutdown_rx.select(future)).ok();
            }).expect("spawn");

        ServerOneConn {
            from_loop: from_loop_rx.wait().unwrap(),
            join_handle: Some(join_handle),
            shutdown_tx: Some(shutdown_tx),
            conn: conn,
        }
    }
}

#[allow(dead_code)]
impl ServerOneConn {
    pub fn port(&self) -> u16 {
        self.from_loop.port
    }

    pub fn dump_state(&self) -> ConnStateSnapshot {
        let g = self.conn.lock().expect("lock");
        let conn = g.as_ref().expect("conn");
        conn.dump_state().wait().expect("dump_status")
    }
}

impl Drop for ServerOneConn {
    fn drop(&mut self) {
        drop(self.shutdown_tx.take().unwrap().send(()));
        self.join_handle.take().unwrap().join().ok();
    }
}
