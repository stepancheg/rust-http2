#[macro_use]
extern crate log;
#[macro_use]
extern crate futures;
extern crate futures_cpupool;

extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_timer;


extern crate tls_api;
extern crate tls_api_stub;
extern crate tokio_tls_api;

extern crate void;
extern crate net2;
extern crate bytes;

pub mod solicit;

pub mod error;

mod result;

pub mod client_conf;
pub mod client_conn;
mod client_tls;
mod service;
mod service_paths;
pub mod client;
pub mod server_conf;
pub mod server_conn;
mod server_tls;
pub mod socket;
pub mod socket_tcp;
pub mod server;

#[cfg(unix)]
extern crate tokio_uds;
#[cfg(unix)]
pub mod socket_unix;

mod ascii;

mod common;
mod client_died_error_holder;

pub mod stream_part;
mod data_or_headers;
// TODO: used in tests; make private
pub mod data_or_headers_with_flag;
pub mod message;

pub mod futures_misc;

mod req_resp;
mod headers_place;

pub mod assert_types;

pub mod hpack;
pub mod solicit_async; // TODO: make private
pub mod solicit_misc;

pub mod misc;
mod rc_mut;

mod resp;

mod exec;

pub use solicit::HttpScheme;
pub use solicit::header::Header;
pub use solicit::header::Headers;

pub use service::Service;
pub use service_paths::ServicePaths;

pub use exec::CpuPoolOption;

pub use client::Client;
pub use client::ClientBuilder;
pub use client_conf::ClientConf;
pub use client_tls::ClientTlsOption;

pub use server::Server;
pub use server::ServerBuilder;
pub use server_conf::ServerConf;
pub use server_conf::ServerAlpn;
pub use server_tls::ServerTlsOption;

pub use resp::Response;
pub use stream_part::HttpPartStreamAfterHeaders;
pub use stream_part::HttpStreamPartAfterHeaders;

pub use error::Error;
pub use error::ErrorCode;
pub use result::Result;

pub mod for_test {
    pub use common::ConnectionStateSnapshot;
    pub use server_conn::ServerConnection;
}
