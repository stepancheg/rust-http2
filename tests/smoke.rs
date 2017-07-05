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
