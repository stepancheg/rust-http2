use httpbis::ClientIntf;
use std::env;
use std::process;
use tokio::runtime::Runtime;

fn main() {
    let rt = Runtime::new().unwrap();

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

    let resp = rt
        .block_on(client.start_get_collect(url.path(), host))
        .expect("execute request");

    print!("{}", resp.dump());
}
