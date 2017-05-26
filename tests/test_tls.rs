//! Test client and server TLS connected with TLS.

extern crate bytes;
extern crate futures;
extern crate native_tls;
extern crate tokio_core;
extern crate tokio_tls;
extern crate httpbis;
extern crate log;
extern crate env_logger;

use bytes::Bytes;

use std::sync::Arc;
use std::net::SocketAddr;

use futures::future::Future;

use httpbis::solicit::header::Headers;
use httpbis::*;
use httpbis::message::SimpleHttpMessage;

use native_tls::TlsAcceptor;
use native_tls::TlsConnector;
use native_tls::Pkcs12;
use native_tls::Certificate;


fn test_tls_acceptor() -> TlsAcceptor {
    let buf = include_bytes!("identity.p12");
    let pkcs12 = Pkcs12::from_der(buf, "mypass").unwrap();
    let builder = TlsAcceptor::builder(pkcs12).unwrap();
    builder.build().unwrap()
}

fn test_tls_connector() -> TlsConnector {
    let root_ca = include_bytes!("root-ca.der");
    let root_ca = Certificate::from_der(root_ca).unwrap();

    let mut builder = TlsConnector::builder().unwrap();
    builder.add_root_certificate(root_ca).expect("add_root_certificate");
    builder.build().unwrap()
}


#[test]
fn tls() {
    struct ServiceImpl {
    }

    impl Service for ServiceImpl {
        fn start_request(&self, _headers: Headers, _req: HttpPartStream) -> Response {
            Response::headers_and_bytes(Headers::ok_200(), Bytes::from("hello"))
        }
    }

    let server = Server::new(
        "[::1]:0".parse::<SocketAddr>().unwrap(),
        ServerTlsOption::Tls(Arc::new(test_tls_acceptor())),
        Default::default(),
        ServiceImpl {});

    let client: Client = Client::new_expl(
        server.local_addr(),
        ClientTlsOption::Tls("foobar.com".to_owned(), Arc::new(test_tls_connector())),
        Default::default())
            .expect("http client");

    let resp: SimpleHttpMessage = client.start_get("/hi", "localhost").collect().wait().unwrap();
    assert_eq!(200, resp.headers.status());
    assert_eq!(&b"hello"[..], &resp.body[..]);
}
