extern crate env_logger;
extern crate httpbis;
extern crate httpbis_h2spec_test;

use std::process;
use std::sync::Arc;

fn main() {
    env_logger::init();

    let mut server = httpbis::ServerBuilder::new_plain();
    server
        .service
        .set_service("/", Arc::new(httpbis_h2spec_test::Ok200));
    server.set_port(8888);
    let _server = server.build().expect("server.build()");

    let mut h2spec = process::Command::new("h2spec")
        .args(&["-p", "8888", "-v"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .spawn()
        .expect("spawn h2spec");

    let exit_status = h2spec.wait().expect("h2spec wait");
    assert!(exit_status.success(), "{}", exit_status);
}
