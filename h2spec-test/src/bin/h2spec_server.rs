extern crate env_logger;
extern crate httpbis;

use std::thread;

fn main() {
    env_logger::init();

    let mut server = httpbis::ServerBuilder::new_plain();
    server.set_port(8888);
    let _server = server.build().expect("server.build()");
    loop {
        thread::park();
    }
}
