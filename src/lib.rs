//! Asynchnous HTTP/2 client and server implementation.
//!
//! Based on futures/tokio.

#[macro_use]
extern crate log;
extern crate log_ndc;

extern crate futures;
extern crate futures_cpupool;

extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_timer;

extern crate tls_api;
extern crate tls_api_stub;
extern crate tokio_tls_api;

extern crate bytes;
extern crate net2;
extern crate rand;
extern crate void;

mod solicit;

mod error;
mod result;

mod client;
mod codec;
mod server;
mod socket;
mod socket_tcp;

#[cfg(unix)]
extern crate tokio_uds;
#[cfg(unix)]
mod socket_unix;

mod ascii;

mod client_died_error_holder;
mod common;

mod data_or_headers;
mod data_or_headers_with_flag;
mod data_or_trailers;
mod message;

mod futures_misc;

mod headers_place;
mod req_resp;

mod assert_types;

mod hpack;
mod solicit_async;
mod solicit_misc;

mod misc;

mod resp;

mod exec;

pub use socket::AnySocketAddr;

pub use solicit::error_code::ErrorCode;
pub use solicit::header::Header;
pub use solicit::header::HeaderName;
pub use solicit::header::HeaderValue;
pub use solicit::header::Headers;
pub use solicit::header::PseudoHeaderName;
pub use solicit::stream_id::StreamId;
pub use solicit::HttpScheme;

pub use exec::_CpuPoolOption;

pub use client::conf::ClientConf;
pub use client::req::ClientRequest;
pub use client::tls::ClientTlsOption;
pub use client::Client;
pub use client::ClientBuilder;
pub use client::ClientInterface;
pub use common::sender::SendError;
pub use common::sender::SenderState;
pub use common::window_size::StreamDead;

pub use server::conf::ServerAlpn;
pub use server::conf::ServerConf;
pub use server::handler::ServerHandler;
pub use server::handler::ServerHandlerContext;
pub use server::handler_paths::ServerHandlerPaths;
pub use server::increase_in_window::ServerIncreaseInWindow;
pub use server::req::ServerRequest;
pub use server::resp::ServerResponse;
pub use server::stream_handler::ServerStreamHandler;
pub use server::tls::ServerTlsOption;
pub use server::Server;
pub use server::ServerBuilder;

pub use data_or_trailers::DataOrTrailers;
pub use data_or_trailers::HttpStreamAfterHeaders;
pub use resp::Response;

pub use message::SimpleHttpMessage;

pub use error::Error;
pub use result::Result;

/// Functions used in tests
#[doc(hidden)]
pub mod for_test {
    pub use common::conn::ConnStateSnapshot;
    pub use common::stream::HttpStreamStateSnapshot;
    pub use server::conn::ServerConn;
    pub use solicit_async::recv_raw_frame_sync;

    pub use solicit::frame::settings::HttpSettings;
    pub use solicit::WindowSize;
    pub use solicit::DEFAULT_SETTINGS;

    pub mod solicit {
        pub use solicit::*;
    }
    pub mod hpack {
        pub use hpack::*;
    }
}
