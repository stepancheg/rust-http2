#[macro_use]
extern crate log;
#[macro_use]
extern crate futures;
extern crate native_tls;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_tls;
extern crate tokio_timer;

extern crate net2;
extern crate bytes;

pub mod solicit;

pub mod error;

mod result;

pub mod client_conf;
pub mod client_conn;
mod client_tls;
mod service;
pub mod client;
pub mod server_conf;
pub mod server_conn;
mod server_tls;
pub mod server;

mod common;

pub mod stream_part;
pub mod message;

pub mod futures_misc;

mod tokio_oneshot;
pub mod assert_types;

pub mod hpack;
pub mod solicit_async; // TODO: make private
pub mod solicit_misc;

pub mod misc;

mod resp;

pub use solicit::HttpScheme;
pub use solicit::header::Header;
pub use solicit::header::Headers;

pub use service::Service;

pub use client::Client;
pub use client_conf::ClientConf;
pub use client_tls::ClientTlsOption;

pub use server::Server;
pub use server_conf::ServerConf;
pub use server_tls::ServerTlsOption;

pub use resp::Response;
pub use stream_part::HttpPartStream;

pub use error::Error;
pub use error::ErrorCode;
pub use result::Result;

pub mod for_test {
    pub use common::ConnectionStateSnapshot;
    pub use server_conn::ServerConnection;
}
