extern crate bytes;
extern crate regex;
extern crate futures;
extern crate httpbis;
extern crate tokio_core;
#[macro_use]
extern crate log;
extern crate env_logger;
#[cfg(unix)]
extern crate tempdir;

use std::sync::Arc;
use std::sync::Mutex;

mod test_misc;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc;

use httpbis::Client;
use httpbis::ServerBuilder;
use httpbis::Service;
use httpbis::Response;
use httpbis::Headers;
use httpbis::HttpPartStream;
use test_misc::*;

#[test]
fn smoke() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let mut futures = Vec::new();
    for _ in 0..10 {
        futures.push(client.start_get("/blocks/200000/5", "localhost").collect());
    }

    let r = future::join_all(futures).wait().expect("wait");
    for rr in r {
        assert_eq!(200000 * 5, rr.body.len());
    }
}

#[cfg(unix)]
#[test]
fn smoke_unix_domain_sockets() {
    env_logger::init().ok();

    let tempdir = tempdir::TempDir::new("rust_http2_test").unwrap();
    let socket_path = tempdir.path().join("test_socket");
    let test_addr = socket_path.to_str().unwrap();

    let _server = ServerTest::new_unix(test_addr.to_owned());

    let client: Client =
        Client::new_plain_unix(
            test_addr,
            Default::default()
        ).expect("client");

    let mut futures = Vec::new();
    for _ in 0..10 {
        futures.push(client.start_get("/blocks/200000/5", "localhost").collect());
    }

    let r = future::join_all(futures).wait().expect("wait");
    for rr in r {
        assert_eq!(200000 * 5, rr.body.len());
    }
}

#[test]
fn parallel_large() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let mut futures = Vec::new();
    for _ in 0..50 {
        futures.push(client.start_get("/blocks/100000/5", "localhost").collect());
    }

    let r = future::join_all(futures).wait().expect("wait");
    for rr in r {
        assert_eq!(100000 * 5, rr.body.len());
    }
}

#[test]
fn seq_long() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let (headers, parts) = client.start_get("/blocks/100000/100", "localhost").0
        .wait().expect("get");

    assert_eq!(200, headers.status());

    let mut sum_len = 0;
    for b in parts.filter_data().wait() {
        sum_len += b.unwrap().len();
    }

    assert_eq!(100000 * 100, sum_len);
}

#[test]
fn seq_slow() {
    env_logger::init().ok();

    let (tx, rx) = mpsc::unbounded();

    struct Handler {
        rx: Mutex<Option<mpsc::UnboundedReceiver<Bytes>>>,
    }

    impl Service for Handler {
        fn start_request(&self, _headers: Headers, _req: HttpPartStream) -> Response {
            let rx = self.rx.lock().unwrap().take().expect("can be called only once");
            Response::headers_and_bytes_stream(
                Headers::ok_200(),
                rx.map_err(|_| unreachable!()))
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service("/", Arc::new(Handler { rx: Mutex::new(Some(rx)) }));
    let server = server.build().expect("server");

    let client: Client =
        Client::new_plain("::1", server.local_addr().port().unwrap(), Default::default()).expect("client");

    let (headers, resp) = client.start_get("/gfgfg", "localhost").0.wait().expect("get");

    assert_eq!(200, headers.status());

    let mut resp = resp.filter_data().wait();

    for i in 1..100 {
        let b = vec![(i % 0x100) as u8; i * 1011];
        tx.send(Bytes::from(&b[..])).expect("send");

        let mut c = Vec::new();
        while c.len() != b.len() {
            c.extend(resp.next().unwrap().unwrap());
        }

        assert_eq!(b, c);
    }

    drop(tx);

    if false {
        // TODO
        assert_eq!(None, resp.next().map(|e| format!("{:?}", e)));
    } else {
        assert_eq!(Some(Ok(Bytes::new())), resp.next().map(|r| r.map_err(|e| format!("{:?}", e))));
    }
}
