#![allow(dead_code)]

use std::sync::Arc;

use futures::stream;

use bytes::Bytes;

use httpbis::Headers;
use httpbis::HttpPartStream;
use httpbis::Response;
use httpbis::Server;
use httpbis::ServerBuilder;
use httpbis::Service;

use regex::Regex;


/// HTTP/2 server used by tests
pub struct ServerTest {
    pub server: Server,
    pub port: u16,
}

struct Blocks {}

impl Service for Blocks {
    fn start_request(&self, headers: Headers, _req: HttpPartStream) -> Response {

        let blocks_re = Regex::new("^/blocks/(\\d+)/(\\d+)$").expect("regex");

        if let Some(captures) = blocks_re.captures(headers.path()) {
            let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");
            let count: u32 = captures.get(2).expect("2").as_str().parse().expect("parse");
            return Response::headers_and_bytes_stream(
                Headers::ok_200(),
                stream::iter_ok((0..count)
                    .map(move |i| Bytes::from(vec![(i % 0xff) as u8; size as usize]))));
        }

        return Response::not_found_404()
    }
}

struct Echo {}

impl Service for Echo {
    fn start_request(&self, _headers: Headers, req: HttpPartStream) -> Response {
        Response::headers_and_stream(Headers::ok_200(), req)
    }
}

impl ServerTest {
    pub fn new() -> ServerTest {
        let mut server = ServerBuilder::new_plain();
        server.set_port(0);
        server.service.set_service("/blocks", Arc::new(Blocks {}));
        server.service.set_service("/echo", Arc::new(Echo {}));
        let server = server.build().expect("server");
        let port = server.local_addr().port().unwrap();
        ServerTest {
            server: server,
            port: port,
        }
    }

    #[cfg(unix)]
    pub fn new_unix(addr: String) -> ServerTest {
        let mut server = ServerBuilder::new_plain_unix();
        server.set_unix_addr(addr).unwrap();

        server.service.set_service("/blocks", Arc::new(Blocks {}));
        server.service.set_service("/echo", Arc::new(Echo {}));
        let server = server.build().expect("server");
        ServerTest {
            server: server,
            port: 0,
        }
    }
}
