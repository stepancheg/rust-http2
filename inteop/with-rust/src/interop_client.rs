extern crate futures;
extern crate httpbis;
extern crate httpbis_interop;
#[macro_use]
extern crate log;
extern crate env_logger;
extern crate clap;

use futures::future::Future;

use httpbis::client::HttpClient;
use httpbis::client_conf::HttpClientConf;

use httpbis_interop::PORT;

use clap::App;
use clap::Arg;

fn not_found(client: HttpClient) {
    let r = client.start_get("/404", "localhost").collect().wait().expect("get");
    assert_eq!(404, r.headers.status());
}

const TESTS: &'static [(&'static str, fn(HttpClient))] = &[
    ("not_found", not_found),
];

fn find_test_case(name: &str) -> Option<fn(HttpClient)> {
    for &(next_name, test) in TESTS {
        if next_name == name {
            return Some(test);
        }
    }
    None
}

fn new_http_client() -> HttpClient {
    HttpClient::new("localhost", PORT, false, HttpClientConf::new()).expect("client")
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
