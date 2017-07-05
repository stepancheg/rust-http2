extern crate bytes;
extern crate regex;
extern crate futures;
extern crate httpbis;
extern crate tokio_core;
#[macro_use]
extern crate log;
extern crate env_logger;

mod test_misc;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;

use httpbis::Client;
use test_misc::*;

#[test]
fn smoke() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let mut futures = Vec::new();
    for _ in 0..10 {
        futures.push(client.start_get("/blocks/200000/5", "localhost").collect());
    }

    let r = future::join_all(futures).wait().expect("wait");
    for rr in r {
        assert_eq!(200000 * 5, rr.body.len());
    }
}

#[test]
fn parallel_large() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let mut futures = Vec::new();
    for _ in 0..50 {
        futures.push(client.start_get("/blocks/100000/5", "localhost").collect());
    }

    let r = future::join_all(futures).wait().expect("wait");
    for rr in r {
        assert_eq!(100000 * 5, rr.body.len());
    }
}

#[test]
fn seq_long() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let client: Client =
        Client::new_plain("::1", server.port, Default::default()).expect("client");

    let (headers, parts) = client.start_get("/blocks/100000/100", "localhost").0
        .wait().expect("get");

    assert_eq!(200, headers.status());

    let mut sum_len = 0;
    for b in parts.filter_data().wait() {
        sum_len += b.unwrap().len();
    }

    assert_eq!(100000 * 100, sum_len);
}
