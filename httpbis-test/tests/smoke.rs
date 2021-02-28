#[macro_use]
extern crate log;

use httpbis_test::*;

use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use futures::future;
use futures::stream::StreamExt;
use tokio::runtime::Runtime;

use httpbis::Client;
use httpbis::ClientIntf;
use httpbis::Headers;
use httpbis::ServerBuilder;
use httpbis::ServerHandler;
use httpbis::ServerRequest;
use httpbis::ServerResponse;
use httpbis::SinkAfterHeaders;
use httpbis::StreamAfterHeaders;

#[test]
fn smoke_tcp_socket() {
    init_logger();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain(BIND_HOST, server.port, Default::default()).expect("client");

    let mut futures = Vec::new();
    for _ in 0..10 {
        futures.push(client.start_get("/blocks/200000/5", "localhost").collect());
    }

    let r = Runtime::new()
        .unwrap()
        .block_on(future::try_join_all(futures))
        .unwrap();
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

    let r = Runtime::new()
        .unwrap()
        .block_on(future::try_join_all(futures))
        .expect("wait");
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

    let r = Runtime::new()
        .unwrap()
        .block_on(future::try_join_all(futures))
        .expect("wait");
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

    let (headers, parts) = Runtime::new()
        .unwrap()
        .block_on(client.start_get("/blocks/100000/100", "localhost"))
        .expect("get");

    assert_eq!(200, headers.status());

    let mut sum_len = 0;

    let rt = Runtime::new().unwrap();

    let mut parts_filter_data = parts.filter_data();

    while let Some(b) = rt.block_on(parts_filter_data.next()) {
        sum_len += b.unwrap().len();
    }

    info!("Done reading response");

    assert_eq!(100000 * 100, sum_len);
}

#[test]
fn seq_slow() {
    init_logger();

    let (tx, rx) = futures::channel::mpsc::unbounded();

    struct Handler {
        rx: Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Bytes>>>,
    }

    impl ServerHandler for Handler {
        fn start_request(&self, _req: ServerRequest, resp: ServerResponse) -> httpbis::Result<()> {
            let rx = self
                .rx
                .lock()
                .unwrap()
                .take()
                .expect("can be called only once");
            let rx = rx.map(Ok);
            let mut sink = resp.send_headers(Headers::ok_200())?;
            sink.pull_bytes_from_stream(rx)?;
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
    )
    .expect("client");

    let rt = Runtime::new().unwrap();

    let (headers, resp) = rt
        .block_on(client.start_get("/gfgfg", "localhost"))
        .expect("get");

    assert_eq!(200, headers.status());

    let mut resp = resp.filter_data();

    for i in 1..100 {
        let b = vec![(i % 0x100) as u8; i * 1011];
        tx.unbounded_send(Bytes::copy_from_slice(&b[..]))
            .expect("send");

        let mut c = Vec::new();
        while c.len() != b.len() {
            c.extend(rt.block_on(resp.next()).unwrap().unwrap());
        }

        assert_eq!(b, c);
    }

    drop(tx);

    if false {
        // TODO
        assert_eq!(None, rt.block_on(resp.next()).map(|e| format!("{:?}", e)));
    } else {
        assert_eq!(
            Some(Ok(Bytes::new())),
            rt.block_on(resp.next())
                .map(|r| r.map_err(|e| format!("{:?}", e)))
        );
    }
}
