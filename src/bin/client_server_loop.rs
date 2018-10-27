extern crate bytes;
extern crate futures;
extern crate httpbis;

use bytes::Bytes;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc;
use httpbis::Client;
use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::Response;
use httpbis::ServerBuilder;
use httpbis::Service;
use std::env;
use std::sync::Arc;
use std::time::Instant;

fn forever(mut cb: impl FnMut()) {
    println!("running forever");

    loop {
        let start = Instant::now();
        let mut limit = 1;
        let mut n = 0;
        loop {
            cb();

            n += 1;
            if n == limit {
                if start.elapsed().as_secs() >= 1 {
                    break;
                }
                limit *= 2;
            }
        }
        let per_iter = start.elapsed() / n;
        let us_per_iter = per_iter.as_secs() * 1_000_000 + per_iter.subsec_micros() as u64;
        println!("{}us per iter", us_per_iter);
    }
}

fn request() {
    struct My;

    impl Service for My {
        fn start_request(&self, _headers: Headers, _req: HttpStreamAfterHeaders) -> Response {
            Response::found_200_plain_text("hello there")
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service("/", Arc::new(My));
    let server = server.build().expect("server");

    let client = Client::new_plain(
        "127.0.0.1",
        server.local_addr().port().unwrap(),
        Default::default(),
    ).expect("client");

    forever(|| {
        let (header, body) = client
            .start_get("/any", "localhost")
            .0
            .wait()
            .expect("headers");
        assert_eq!(200, header.status());

        // TODO: check content
        body.collect().wait().expect("body");
    });
}

fn ping_pong() {
    const BLOCK_SIZE: usize = 1000;

    struct Echo;

    impl Service for Echo {
        fn start_request(&self, _headers: Headers, req: HttpStreamAfterHeaders) -> Response {
            Response::headers_and_bytes_stream(Headers::ok_200(), req.filter_data())
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service("/", Arc::new(Echo));
    let server = server.build().expect("server");

    let client = Client::new_plain(
        "127.0.0.1",
        server.local_addr().port().unwrap(),
        Default::default(),
    ).expect("client");

    let (request_tx, request_rx) = mpsc::unbounded();

    let (header, body) = client
        .start_post_stream(
            "/any",
            "localhost",
            request_rx.map_err(|()| httpbis::Error::Other("other")),
        ).wait()
        .expect("request");

    assert_eq!(200, header.status());

    let body = body.filter_data();
    let mut body = body.wait();

    let mut i = 0u32;
    forever(|| {
        i = i.wrapping_add(1);
        let mut req = Vec::new();
        req.resize(BLOCK_SIZE, i as u8);
        request_tx
            .unbounded_send(Bytes::from(req))
            .expect("send error");

        let mut read = 0;
        while read < BLOCK_SIZE {
            let chunk = body.next().unwrap().unwrap();
            read += chunk.len();
        }
        assert_eq!(BLOCK_SIZE, read);
    });
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match &args[1..] {
        &["request"] => request(),
        &["ping-pong"] => ping_pong(),
        _ => panic!("usage: {} <mode>"),
    }
}
