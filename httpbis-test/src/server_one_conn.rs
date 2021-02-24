use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use futures::channel::oneshot;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;

use httpbis;
use httpbis::for_test::*;
use httpbis::*;

use super::BIND_HOST;
use crate::assert_types::assert_send_future;
use std::thread::JoinHandle;
use tokio::runtime::Runtime;

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
        let (from_loop_tx, from_loop_rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let conn: Arc<Mutex<Option<ServerConn>>> = Default::default();

        let conn_for_thread = conn.clone();

        let join_handle: JoinHandle<()> = thread::Builder::new()
            .name("server_one_conn".to_owned())
            .spawn(move || {
                let lp = Runtime::new().unwrap();

                let listener = lp
                    .block_on(tokio::net::TcpListener::bind(
                        &(BIND_HOST, port).to_socket_addrs().unwrap().next().unwrap(),
                    ))
                    .unwrap();

                let actual_port = listener.local_addr().unwrap().port();
                from_loop_tx
                    .send(FromLoop { port: actual_port })
                    .ok()
                    .unwrap();

                let handle = lp.handle().clone();

                let conn = listener.accept().map_err(httpbis::Error::from);

                // TODO: close listening port
                //drop(listener);

                let future = async move {
                    let (conn, peer_addr) = match conn.await {
                        Ok((conn, peer_addr)) => (conn, peer_addr),
                        Err(e) => {
                            warn!("accept failed: {}", e);
                            return;
                        }
                    };

                    let (conn, future) = ServerConn::new_plain_single_thread_fn(
                        &handle,
                        conn,
                        peer_addr,
                        Default::default(),
                        service,
                    );
                    *conn_for_thread.lock().unwrap() = Some(conn);
                    future.await
                };

                let shutdown_rx = shutdown_rx.then(|_| future::ready(()));

                let shutdown_rx = assert_send_future::<(), _>(shutdown_rx);

                let shutdown_rx = Box::pin(shutdown_rx);

                lp.block_on(future::select(shutdown_rx, Box::pin(future)));
            })
            .expect("spawn");

        ServerOneConn {
            from_loop: from_loop_rx.recv().unwrap(),
            join_handle: Some(join_handle),
            shutdown_tx: Some(shutdown_tx),
            conn,
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
        Runtime::new()
            .unwrap()
            .block_on(conn.dump_state())
            .expect("dump_status")
    }
}

impl Drop for ServerOneConn {
    fn drop(&mut self) {
        drop(self.shutdown_tx.take().unwrap().send(()));
        self.join_handle.take().unwrap().join().ok();
    }
}
