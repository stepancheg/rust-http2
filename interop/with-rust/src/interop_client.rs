extern crate bytes;
extern crate futures;
extern crate httpbis;
extern crate httpbis_interop;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate clap;
extern crate tls_api;
extern crate tls_api_openssl;

use std::sync::Arc;
use std::net::SocketAddr;
use std::net::IpAddr;

use bytes::Bytes;

use tls_api::Certificate;
use tls_api_openssl::TlsConnector;
use tls_api::TlsConnector as tls_api_TlsConnector;
use tls_api::TlsConnectorBuilder;

use futures::future::Future;

use httpbis::Client;
use httpbis::ClientConf;
use httpbis::ClientTlsOption;

use httpbis_interop::PORT;

use clap::App;
use clap::Arg;

fn not_found(client: Client) {
    let r = client.start_get("/404", "localhost").collect().wait().expect("get");
    assert_eq!(404, r.headers.status());
}

fn found(client: Client) {
    let r = client.start_get("/200", "localhost").collect().wait().expect("get");
    assert_eq!(200, r.headers.status());
    assert_eq!(Bytes::from("200 200 200"), r.body);
}

const TESTS: &'static [(&'static str, fn(Client))] = &[
    ("not_found", not_found),
    ("found",     found),
];

fn find_test_case(name: &str) -> Option<fn(Client)> {
    for &(next_name, test) in TESTS {
        if next_name == name {
            return Some(test);
        }
    }
    None
}

fn test_tls_connector() -> TlsConnector {
    let root_ca = include_bytes!("../../root-ca.der");
    let root_ca = Certificate::from_der(root_ca.to_vec());

    let mut builder = TlsConnector::builder().unwrap();
    builder.add_root_certificate(root_ca).expect("add_root_certificate");
    builder.build().unwrap()
}

fn new_http_client() -> Client {
    Client::new_expl(
        &SocketAddr::from(("::1".parse::<IpAddr>().unwrap(), PORT)),
        ClientTlsOption::Tls("foobar.com".to_owned(), Arc::new(test_tls_connector())),
        ClientConf::new())
            .expect("client")
}

fn run_test_case(name: &str) {
    let f = find_test_case(name).expect(&format!("test case not found: {}", name));
    f(new_http_client());
}

fn run_all_test_cases() {
    for &(name, test) in TESTS {
        info!("starting test {}", name);
        test(new_http_client());
    }
}

fn main() {
    env_logger::init().expect("env_logger::init");

    let options = App::new("http2 interop client")
        .arg(Arg::with_name("test_case")
            .long("test_case")
            .help("The name of the test case to execute. For example, \"not_found\"")
            .takes_value(true))
        .get_matches();

    match options.value_of("test_case") {
        Some(test_case) => {
            run_test_case(test_case);
        }
        None => {
            run_all_test_cases();
        }
    }
}
