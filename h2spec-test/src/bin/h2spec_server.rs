extern crate env_logger;
extern crate httpbis;
extern crate httpbis_h2spec_test;

use std::sync::Arc;
use std::thread;

fn main() {
    env_logger::init();

    let mut server = httpbis::ServerBuilder::new_plain();
    server
        .service
        .set_service("/", Arc::new(httpbis_h2spec_test::Ok200));
    server.set_port(8888);
    let _server = server.build().expect("server.build()");
    loop {
        thread::park();
    }
}
