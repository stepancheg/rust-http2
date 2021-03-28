//! Test client and server TLS connected with TLS.

use httpbis_test::*;

use std::sync::Arc;

use httpbis::SimpleHttpMessage;
use httpbis::*;

use httpbis::AnySocketAddr;

use tls_api::TlsAcceptor as tls_api_TlsAcceptor;
use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;
use tls_api::TlsConnector as tls_api_TlsConnector;
use tls_api::TlsConnectorBuilder;
use tls_api_native_tls::TlsAcceptor;
use tls_api_native_tls::TlsConnector;
use tokio::runtime::Runtime;

fn test_tls_acceptor() -> TlsAcceptor {
    let server_keys = &test_cert_gen::keys().server;

    let builder = TlsAcceptor::builder_from_pkcs12(
        &server_keys.cert_and_key_pkcs12.pkcs12.0,
        &server_keys.cert_and_key_pkcs12.password,
    )
    .unwrap();
    builder.build().unwrap()
}

fn test_tls_connector() -> TlsConnector {
    let client_keys = &test_cert_gen::keys().client;

    let mut builder = TlsConnector::builder().unwrap();
    builder
        .add_root_certificate(client_keys.ca.get_der())
        .expect("add_root_certificate");
    builder.build().unwrap()
}

#[test]
fn tls() {
    init_logger();

    let rt = Runtime::new().unwrap();

    struct ServiceImpl {}

    impl ServerHandler for ServiceImpl {
        fn start_request(&self, _req: ServerRequest, resp: ServerResponse) -> httpbis::Result<()> {
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
        ClientTlsOption::Tls(
            "localhost".to_owned(),
            Arc::new(test_tls_connector().into_dyn()),
        ),
        Default::default(),
    )
    .expect("http client");

    let resp: SimpleHttpMessage = rt
        .block_on(client.start_get_collect("/hi", "localhost"))
        .unwrap();
    assert_eq!(200, resp.headers.status());
    assert_eq!(&b"hello"[..], resp.body.as_ref());
}
