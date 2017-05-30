#![allow(dead_code)]

use futures::stream;

use bytes::Bytes;

use httpbis;
use httpbis::server::Server;
use httpbis::server::ServerTlsOption;
use httpbis::*;

use regex::Regex;


/// HTTP/2 server used by tests
pub struct HttpServerTest {
    pub server: Server,
    pub port: u16,
}

struct TestService {
}

impl httpbis::Service for TestService {
    fn start_request(&self, headers: Headers, req: httpbis::HttpPartStream) -> Response {

        let blocks_re = Regex::new("^/blocks/(\\d+)/(\\d+)$").expect("regex");
        let echo_re = Regex::new("^/echo$").expect("regex");

        if let Some(captures) = blocks_re.captures(headers.path()) {
            let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");
            let count: u32 = captures.get(2).expect("2").as_str().parse().expect("parse");
            return Response::headers_and_bytes_stream(
                Headers::ok_200(),
                stream::iter((0..count)
                    .map(move |i| Ok(Bytes::from(vec![(i % 0xff) as u8; size as usize])))));
        }

        if let Some(_) = echo_re.captures(headers.path()) {
            return Response::headers_and_stream(Headers::ok_200(), req);
        }

        return Response::not_found_404()
    }
}

impl HttpServerTest {
    pub fn new() -> HttpServerTest {
        let http_server = Server::new("[::1]:0", ServerTlsOption::Plain, Default::default(), TestService {});
        let port = http_server.local_addr().port();
        HttpServerTest {
            server: http_server,
            port: port,
        }
    }
}
