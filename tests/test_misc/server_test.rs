#![allow(dead_code)]

use httpbis;
use httpbis::server::Server;
use httpbis::server::ServerTlsOption;
use httpbis::*;

use regex::Regex;


/// HTTP/2 server used by tests
pub struct HttpServerTest {
    server: Server,
    pub port: u16,
}

struct TestService {
}

impl httpbis::Service for TestService {
    fn start_request(&self, headers: Headers, req: httpbis::HttpPartStream) -> Response {

        let blocks_re = Regex::new("^/blocks/(\\d+)/(\\d+)$").expect("regex");
        let echo_re = Regex::new("^/echo$").expect("regex");

        if let Some(_captures) = blocks_re.captures(headers.path()) {
            unimplemented!();
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
