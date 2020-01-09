extern crate env_logger;
extern crate httpbis;
extern crate httpbis_h2spec_test;

use std::io;
use std::sync::Arc;

fn main() {
    log_ndc_env_logger::init();

    let mut server = httpbis::ServerBuilder::new_plain();
    server
        .service
        .set_service("/", Arc::new(httpbis_h2spec_test::Ok200));
    server.set_port(8888);
    let server = server.build().expect("server.build()");
    loop {
        let mut line = String::new();
        io::stdin().read_line(&mut line).unwrap();

        let state = futures::executor::block_on(server.dump_state()).unwrap();
        println!("{:#?}", state);
    }
}
