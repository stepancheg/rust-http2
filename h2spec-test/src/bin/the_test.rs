extern crate httpbis;
extern crate env_logger;

use std::process;

fn main() {
    env_logger::init();

    let mut server = httpbis::ServerBuilder::new_plain();
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
