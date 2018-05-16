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

mod solicit;

mod error;
mod result;

mod client_conf;
mod client_conn;
mod client_tls;
mod service;
mod service_paths;
mod client;
mod server_conf;
mod server_conn;
mod server_tls;
mod socket;
mod socket_tcp;
mod server;

#[cfg(unix)]
extern crate tokio_uds;
#[cfg(unix)]
mod socket_unix;

mod ascii;

mod common;
mod client_died_error_holder;

mod data_or_trailers;
mod data_or_headers;
// TODO: used in tests; make private
pub mod data_or_headers_with_flag;
pub mod message;

pub mod futures_misc;

mod req_resp;
mod headers_place;

mod assert_types;

mod hpack;
mod solicit_async;
mod solicit_misc;

pub mod misc;
mod rc_mut;

mod resp;

mod exec;

pub use socket::AnySocketAddr;

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

pub use data_or_trailers::DataOrTrailers;
pub use data_or_trailers::HttpStreamAfterHeaders;
pub use resp::Response;

pub use error::Error;
pub use error::ErrorCode;
pub use result::Result;

/// Functions used in tests
#[doc(hidden)]
pub mod for_test {
    pub use common::ConnectionStateSnapshot;
    pub use server_conn::ServerConnection;
    pub use solicit_async::recv_raw_frame_sync;
    pub mod solicit {
        pub use ::solicit::*;
    }
}
