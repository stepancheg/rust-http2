extern crate futures;
extern crate httpbis;
extern crate tls_api;
extern crate tls_api_openssl;
extern crate url;

use std::env;
use std::process;

use futures::future::Future;

fn main() {
    let args = env::args();
    let args: Vec<_> = args.collect();
    if args.len() != 2 {
        println!("usage: {} <url>", &args[0]);
        process::exit(1);
    }

    let url = &args[1];
    let url = url::Url::parse(&url).expect("parse url");

    if url.scheme() != "https" {
        panic!("URL scheme must be https");
    }

    let host = url.host_str().expect("URL must have host");
    let port = url.port().unwrap_or(443);

    let client =
        httpbis::Client::new_tls::<tls_api_openssl::TlsConnector>(host, port, Default::default())
            .expect("client");

    let resp = client
        .start_get(url.path(), host)
        .collect()
        .wait()
        .expect("execute request");

    print!("{}", resp.dump());
}
