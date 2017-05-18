#![allow(dead_code)]

use httpbis;
use httpbis::server::Server;
use httpbis::server::ServerTlsOption;
use httpbis::*;


pub struct HttpServerEcho {
    server: Server,
    pub port: u16,
}

struct EchoService {
}

impl httpbis::Service for EchoService {
    fn start_request(&self, _headers: Headers, req: httpbis::HttpPartStream) -> Response {
        let headers = Headers::ok_200();
        Response::headers_and_stream(headers, req)
    }
}

impl HttpServerEcho {
    pub fn new() -> HttpServerEcho {
        let http_server = Server::new("[::1]:0", ServerTlsOption::Plain, Default::default(), EchoService {});
        let port = http_server.local_addr().port();
        HttpServerEcho {
            server: http_server,
            port: port,
        }
    }
}
