extern crate httpbis;
extern crate httpbis_interop;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate tls_api;
extern crate tls_api_native_tls;

use std::sync::Arc;
use std::thread;

use tls_api_native_tls::TlsAcceptor;
use tls_api_native_tls::TlsAcceptorBuilder;
use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;

use httpbis::message::SimpleHttpMessage;
use httpbis::Headers;
use httpbis::Response;
use httpbis::Server;
use httpbis::ServerTlsOption;
use httpbis::ServerConf;
use httpbis::HttpPartStream;
use httpbis_interop::PORT;

struct ServiceImpl {
}

impl httpbis::Service for ServiceImpl {
    fn start_request(&self, headers: Headers, _req: HttpPartStream) -> Response {
        info!("request: {:?}", headers);

        if headers.path() == "/200" {
            Response::message(SimpleHttpMessage::found_200_plain_text("200 200 200"))
        } else {
            Response::message(SimpleHttpMessage::not_found_404("not found"))
        }
    }
}

fn test_tls_acceptor() -> TlsAcceptor {
    let pkcs12 = include_bytes!("../../identity.p12");
    let builder = TlsAcceptorBuilder::from_pkcs12(pkcs12, "mypass").unwrap();
    builder.build().unwrap()
}

fn main() {
    env_logger::init().expect("env_logger::init");

    let _server = Server::new(
        ("::", PORT),
        ServerTlsOption::Tls(Arc::new(test_tls_acceptor())),
        ServerConf::new(),
        ServiceImpl {});

    loop {
        thread::park();
    }
}
