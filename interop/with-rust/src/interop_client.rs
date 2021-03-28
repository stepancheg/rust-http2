#[macro_use]
extern crate log;

use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;

use tls_api::TlsConnector as tls_api_TlsConnector;
use tls_api::TlsConnectorBuilder;
use tls_api_openssl::TlsConnector;

use httpbis::Client;
use httpbis::ClientConf;
use httpbis::ClientIntf;
use httpbis::ClientTlsOption;

use httpbis_interop::PORT;

use bytes::Bytes;
use clap::App;
use clap::Arg;
use tokio::runtime::Runtime;

fn not_found(client: Client) {
    let rt = Runtime::new().unwrap();
    let r = rt
        .block_on(client.start_get_collect("/404", "localhost"))
        .expect("get");
    assert_eq!(404, r.headers.status());
}

fn found(client: Client) {
    let rt = Runtime::new().unwrap();
    let r = rt
        .block_on(client.start_get_collect("/200", "localhost"))
        .expect("get");
    assert_eq!(200, r.headers.status());
    assert_eq!(Bytes::from("200 200 200"), r.body);
}

const TESTS: &'static [(&'static str, fn(Client))] = &[("not_found", not_found), ("found", found)];

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

    let mut builder = TlsConnector::builder().unwrap();
    builder
        .add_root_certificate(root_ca)
        .expect("add_root_certificate");
    builder.build().unwrap()
}

fn new_http_client() -> Client {
    Client::new_expl(
        &SocketAddr::from(("127.0.0.1".parse::<IpAddr>().unwrap(), PORT)),
        ClientTlsOption::Tls(
            "foobar.com".to_owned(),
            Arc::new(test_tls_connector().into_dyn()),
        ),
        ClientConf::new(),
    )
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
    log_ndc_env_logger::init();

    let options = App::new("http2 interop client")
        .arg(
            Arg::with_name("test_case")
                .long("test_case")
                .help("The name of the test case to execute. For example, \"not_found\"")
                .takes_value(true),
        )
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
