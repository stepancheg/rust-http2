extern crate bytes;
extern crate env_logger;
extern crate futures;
extern crate httpbis;
extern crate log;
extern crate regex;
#[cfg(unix)]
extern crate tempdir;
extern crate tokio_core;

extern crate httpbis_test;
use httpbis_test::*;

use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc;

use httpbis::Client;
use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::ServerBuilder;
use httpbis::ServerHandler;
use httpbis::ServerHandlerContext;
use httpbis::ServerResponse;

#[test]
fn smoke() {
    init_logger();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain(BIND_HOST, server.port, Default::default()).expect("client");

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
    init_logger();

    let tempdir = tempdir::TempDir::new("rust_http2_test").unwrap();
    let socket_path = tempdir.path().join("test_socket");
    let test_addr = socket_path.to_str().unwrap();

    let _server = ServerTest::new_unix(test_addr.to_owned());

    let client: Client = Client::new_plain_unix(test_addr, Default::default()).expect("client");

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
    init_logger();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain(BIND_HOST, server.port, Default::default()).expect("client");

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
    init_logger();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain(BIND_HOST, server.port, Default::default()).expect("client");

    let (headers, parts) = client
        .start_get("/blocks/100000/100", "localhost")
        .0
        .wait()
        .expect("get");

    assert_eq!(200, headers.status());

    let mut sum_len = 0;
    for b in parts.filter_data().wait() {
        sum_len += b.unwrap().len();
    }

    assert_eq!(100000 * 100, sum_len);
}

#[test]
fn seq_slow() {
    init_logger();

    let (tx, rx) = mpsc::unbounded();

    struct Handler {
        rx: Mutex<Option<mpsc::UnboundedReceiver<Bytes>>>,
    }

    impl ServerHandler for Handler {
        fn start_request(
            &self,
            _context: ServerHandlerContext,
            _headers: Headers,
            _req: HttpStreamAfterHeaders,
            mut resp: ServerResponse,
        ) -> httpbis::Result<()> {
            let rx = self
                .rx
                .lock()
                .unwrap()
                .take()
                .expect("can be called only once");
            let rx = rx.map_err(|()| unreachable!());
            resp.send_headers(Headers::ok_200())?;
            resp.pull_bytes_from_stream(rx)?;
            Ok(())
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service(
        "/",
        Arc::new(Handler {
            rx: Mutex::new(Some(rx)),
        }),
    );
    let server = server.build().expect("server");

    let client: Client = Client::new_plain(
        BIND_HOST,
        server.local_addr().port().unwrap(),
        Default::default(),
    ).expect("client");

    let (headers, resp) = client
        .start_get("/gfgfg", "localhost")
        .0
        .wait()
        .expect("get");

    assert_eq!(200, headers.status());

    let mut resp = resp.filter_data().wait();

    for i in 1..100 {
        let b = vec![(i % 0x100) as u8; i * 1011];
        tx.unbounded_send(Bytes::from(&b[..])).expect("send");

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
        assert_eq!(
            Some(Ok(Bytes::new())),
            resp.next().map(|r| r.map_err(|e| format!("{:?}", e)))
        );
    }
}
