//! Test client and server TLS connected with TLS.

extern crate bytes;
extern crate futures;
extern crate httpbis;
extern crate regex;
extern crate tls_api;
extern crate tls_api_native_tls;

extern crate httpbis_test;
use httpbis_test::*;

use std::sync::Arc;

use httpbis::SimpleHttpMessage;
use httpbis::*;

use httpbis::AnySocketAddr;

use tls_api::Certificate;
use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;
use tls_api::TlsConnector as tls_api_TlsConnector;
use tls_api::TlsConnectorBuilder;
use tls_api_native_tls::TlsAcceptor;
use tls_api_native_tls::TlsAcceptorBuilder;
use tls_api_native_tls::TlsConnector;
use tokio::runtime::Runtime;

fn test_tls_acceptor() -> TlsAcceptor {
    let server_keys = &httpbis_test::openssl_test_key_gen::keys().server;

    let builder =
        TlsAcceptorBuilder::from_pkcs12(&server_keys.pkcs12, &server_keys.pkcs12_password).unwrap();
    builder.build().unwrap()
}

fn test_tls_connector() -> TlsConnector {
    let client_keys = &httpbis_test::openssl_test_key_gen::keys().client;

    let mut builder = TlsConnector::builder().unwrap();
    builder
        .add_root_certificate(Certificate::from_der(client_keys.cert_der.clone()))
        .expect("add_root_certificate");
    builder.build().unwrap()
}

#[test]
fn tls() {
    init_logger();

    let mut rt = Runtime::new().unwrap();

    struct ServiceImpl {}

    impl ServerHandler for ServiceImpl {
        fn start_request(
            &self,
            _context: ServerHandlerContext,
            _req: ServerRequest,
            mut resp: ServerResponse,
        ) -> httpbis::Result<()> {
            resp.send_found_200_plain_text("hello")?;
            Ok(())
        }
    }

    let mut server = ServerBuilder::new();
    server.set_addr((BIND_HOST, 0)).expect("set_addr");
    server.set_tls(test_tls_acceptor());
    server.service.set_service("/", Arc::new(ServiceImpl {}));
    let server = server.build().expect("server");

    let socket_addr = match server.local_addr() {
        &AnySocketAddr::Inet(ref sock_addr) => sock_addr,
        _ => panic!("Assumed server was an inet server"),
    };

    let client: Client = Client::new_expl(
        socket_addr,
        ClientTlsOption::Tls("localhost".to_owned(), Arc::new(test_tls_connector())),
        Default::default(),
    )
    .expect("http client");

    let resp: SimpleHttpMessage = rt
        .block_on(client.start_get("/hi", "localhost").collect())
        .unwrap();
    assert_eq!(200, resp.headers.status());
    assert_eq!(&b"hello"[..], resp.body.get_bytes());
}
