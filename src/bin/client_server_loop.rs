extern crate futures;
extern crate httpbis;

use futures::future::Future;
use futures::stream::Stream;
use httpbis::Client;
use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::Response;
use httpbis::ServerBuilder;
use httpbis::Service;
use std::sync::Arc;
use std::time::Instant;

fn main() {
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

    println!("running forever");

    loop {
        let start = Instant::now();
        let mut limit = 1;
        let mut n = 0;
        loop {
            let (header, body) = client
                .start_get("/any", "localhost")
                .0
                .wait()
                .expect("headers");
            assert_eq!(200, header.status());

            // TODO: check content
            body.collect().wait().expect("body");

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
