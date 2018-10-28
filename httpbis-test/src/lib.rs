#[macro_use]
extern crate log;
extern crate env_logger;

extern crate regex;

extern crate bytes;

extern crate futures;

extern crate tokio_core;

extern crate httpbis;

use std::sync::Once;

mod client;
#[path = "../../src/misc.rs"]
mod misc;
mod server_one_conn;
mod server_test;
mod tester;

pub use self::server_one_conn::*;
pub use self::server_test::*;
pub use self::tester::*;
pub use client::*;
pub use misc::*;

// Bind on IPv4 because IPv6 is broken on travis
pub const BIND_HOST: &str = "127.0.0.1";

pub fn init_logger() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        env_logger::init();
    });
}
