extern crate httpbis;
extern crate httpbis_interop;
extern crate env_logger;

use std::thread;

use httpbis::http_common::HttpService;
use httpbis::message::SimpleHttpMessage;
use httpbis::Headers;
use httpbis::HttpResponse;
use httpbis::server::HttpServer;
use httpbis::server::ServerTlsOption;
use httpbis::server_conf::HttpServerConf;
use httpbis::http_common::HttpPartStream;
use httpbis_interop::PORT;

struct ServiceImpl {
}

impl HttpService for ServiceImpl {
    fn start_request(&self, _headers: Headers, _req: HttpPartStream) -> HttpResponse {
        HttpResponse::message(SimpleHttpMessage::not_found_404("not found"))
    }
}

fn main() {
    env_logger::init().expect("env_logger::init");

    let _server = HttpServer::new(
        ("::", PORT),
        ServerTlsOption::Plain,
        HttpServerConf::new(),
        ServiceImpl {});

    loop {
        thread::park();
    }
}
