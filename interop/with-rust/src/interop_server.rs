extern crate futures;
extern crate bytes;
extern crate log;
extern crate env_logger;
extern crate tls_api;
extern crate tls_api_openssl;
extern crate regex;

extern crate httpbis;
extern crate httpbis_interop;

use std::sync::Arc;
use std::thread;

use bytes::Bytes;

use futures::stream;

use regex::Regex;

use tls_api_openssl::TlsAcceptor;
use tls_api_openssl::TlsAcceptorBuilder;
use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;

use httpbis::message::SimpleHttpMessage;
use httpbis::Headers;
use httpbis::Response;
use httpbis::Service;
use httpbis::ServerBuilder;
use httpbis::HttpPartStreamAfterHeaders;
use httpbis_interop::PORT;

struct Found200 {}

impl Service for Found200 {
    fn start_request(&self, _headers: Headers, _req: HttpPartStreamAfterHeaders) -> Response {
        Response::message(SimpleHttpMessage::found_200_plain_text("200 200 200"))
    }
}

struct Blocks {}

impl Service for Blocks {
    fn start_request(&self, headers: Headers, _req: HttpPartStreamAfterHeaders) -> Response {
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

fn test_tls_acceptor() -> TlsAcceptor {
    let pkcs12 = include_bytes!("../../identity.p12");
    let mut builder = TlsAcceptorBuilder::from_pkcs12(pkcs12, "mypass").unwrap();
    builder.set_alpn_protocols(&[b"h2"]).expect("set_alpn_protocols");
    builder.build().unwrap()
}

fn main() {
    env_logger::init();

    let mut server = ServerBuilder::new();
    server.set_port(PORT);
    server.set_tls(test_tls_acceptor());
    server.service.set_service("/200", Arc::new(Found200 {}));
    server.service.set_service("/blocks", Arc::new(Blocks {}));
    let _server = server.build().expect("server");

    loop {
        thread::park();
    }
}
