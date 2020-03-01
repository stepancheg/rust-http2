#[macro_use]
extern crate log;
extern crate log_ndc_env_logger;

extern crate regex;

extern crate bytes;

extern crate futures;

extern crate httpbis;

use std::sync::Once;

#[macro_use]
mod t;

mod assert_types;
mod bytes_ext;
mod client;
#[path = "../../src/misc.rs"]
mod misc;
pub mod openssl_test_key_gen;
mod server_one_conn;
mod server_test;
mod task;
mod tester;

pub use self::server_one_conn::*;
pub use self::server_test::*;
pub use self::tester::*;
pub use client::*;
pub use misc::*;
pub use task::*;

// Bind on IPv4 because IPv6 is broken on travis
pub const BIND_HOST: &str = "127.0.0.1";

pub fn init_logger() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log_ndc_env_logger::init();
    });
}
