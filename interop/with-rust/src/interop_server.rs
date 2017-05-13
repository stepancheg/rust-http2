extern crate httpbis;
extern crate httpbis_interop;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate native_tls;

use std::sync::Arc;
use std::thread;

use native_tls::TlsAcceptor;
use native_tls::Pkcs12;

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
    fn start_request(&self, headers: Headers, _req: HttpPartStream) -> HttpResponse {
        info!("request: {:?}", headers);

        if headers.path() == "/200" {
            HttpResponse::message(SimpleHttpMessage::found_200_plain_text("200 200 200"))
        } else {
            HttpResponse::message(SimpleHttpMessage::not_found_404("not found"))
        }
    }
}

fn test_tls_acceptor() -> TlsAcceptor {
    let buf = include_bytes!("../../identity.p12");
    let pkcs12 = Pkcs12::from_der(buf, "mypass").unwrap();
    let builder = TlsAcceptor::builder(pkcs12).unwrap();
    builder.build().unwrap()
}

fn main() {
    env_logger::init().expect("env_logger::init");

    let _server = HttpServer::new(
        ("::", PORT),
        ServerTlsOption::Tls(Arc::new(test_tls_acceptor())),
        HttpServerConf::new(),
        ServiceImpl {});

    loop {
        thread::park();
    }
}
