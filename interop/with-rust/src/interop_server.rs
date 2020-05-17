use std::sync::Arc;
use std::thread;

use bytes::Bytes;

use futures::stream;

use regex::Regex;

use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;
use tls_api_openssl::TlsAcceptor;
use tls_api_openssl::TlsAcceptorBuilder;

use httpbis::ServerBuilder;
use httpbis::ServerHandler;
use httpbis::ServerHandlerContext;
use httpbis::ServerRequest;
use httpbis::ServerResponse;
use httpbis_interop::PORT;

struct Found200 {}

impl ServerHandler for Found200 {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        _req: ServerRequest,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        resp.send_found_200_plain_text("200 200 200")?;
        Ok(())
    }
}

struct Blocks {}

impl ServerHandler for Blocks {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        req: ServerRequest,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        let blocks_re = Regex::new("^/blocks/(\\d+)/(\\d+)$").expect("regex");

        if let Some(captures) = blocks_re.captures(req.headers.path()) {
            let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");
            let count: u32 = captures.get(2).expect("2").as_str().parse().expect("parse");
            let stream = stream::iter(
                (0..count).map(move |i| Ok(Bytes::from(vec![(i % 0xff) as u8; size as usize]))),
            );
            resp.pull_bytes_from_stream(stream)?;
        } else {
            resp.send_not_found_404("expecting /blocks/")?;
        }

        Ok(())
    }
}

fn test_tls_acceptor() -> TlsAcceptor {
    let pkcs12 = include_bytes!("../../identity.p12");
    let mut builder = TlsAcceptorBuilder::from_pkcs12(pkcs12, "mypass").unwrap();
    builder
        .set_alpn_protocols(&[b"h2"])
        .expect("set_alpn_protocols");
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
