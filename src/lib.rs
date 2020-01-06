//! Asynchnous HTTP/2 client and server implementation.
//!
//! Based on futures/tokio.

#[macro_use]
extern crate log;
extern crate log_ndc;

extern crate tls_api;
extern crate tls_api_stub;

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

mod display_comma_separated;
mod misc;

mod resp;

mod exec;

mod log_ndc_future;

pub use crate::socket::AnySocketAddr;

pub use crate::solicit::error_code::ErrorCode;
pub use crate::solicit::header::Header;
pub use crate::solicit::header::HeaderName;
pub use crate::solicit::header::HeaderValue;
pub use crate::solicit::header::Headers;
pub use crate::solicit::header::PseudoHeaderName;
pub use crate::solicit::stream_id::StreamId;
pub use crate::solicit::HttpScheme;

pub use crate::client::conf::ClientConf;
pub use crate::client::req::ClientRequest;
pub use crate::client::tls::ClientTlsOption;
pub use crate::client::Client;
pub use crate::client::ClientBuilder;
pub use crate::client::ClientInterface;
pub use crate::common::sender::SendError;
pub use crate::common::sender::SenderState;
pub use crate::common::window_size::StreamDead;

pub use crate::server::conf::ServerAlpn;
pub use crate::server::conf::ServerConf;
pub use crate::server::handler::ServerHandler;
pub use crate::server::handler::ServerHandlerContext;
pub use crate::server::handler_paths::ServerHandlerPaths;
pub use crate::server::increase_in_window::ServerIncreaseInWindow;
pub use crate::server::req::ServerRequest;
pub use crate::server::resp::ServerResponse;
pub use crate::server::stream_handler::ServerStreamHandler;
pub use crate::server::tls::ServerTlsOption;
pub use crate::server::Server;
pub use crate::server::ServerBuilder;

pub use crate::data_or_trailers::DataOrTrailers;
pub use crate::data_or_trailers::HttpStreamAfterHeaders;
pub use crate::resp::Response;

pub use crate::message::SimpleHttpMessage;

pub use crate::error::Error;
pub use crate::result::Result;

/// Functions used in tests
#[doc(hidden)]
pub mod for_test {
    pub use crate::common::conn::ConnStateSnapshot;
    pub use crate::common::stream::HttpStreamStateSnapshot;
    pub use crate::server::conn::ServerConn;
    pub use crate::solicit_async::recv_raw_frame_sync;

    pub use crate::solicit::frame::settings::HttpSettings;
    pub use crate::solicit::WindowSize;
    pub use crate::solicit::DEFAULT_SETTINGS;

    pub mod solicit {
        pub use crate::solicit::*;
    }
    pub mod hpack {
        pub use crate::hpack::*;
    }
}
