#![deny(broken_intra_doc_links)]
// TODO: add docs
//#![deny(missing_docs)]

//! Asynchnous HTTP/2 client and server implementation.
//!
//! Based on tokio.
//!
//! This crate is used to implement [`grpc` crate](https://github.com/stepancheg/grpc-rust),
//! and probably not usable for anything else.

#[macro_use]
extern crate log;

pub use bytes_ext::buf_get_bytes::BufGetBytes;
pub use bytes_ext::bytes_deque::BytesDeque;
pub use client::conf::ClientConf;
pub use client::handler::ClientHandler;
pub use client::req::ClientRequest;
pub use client::resp_future::ClientResponseFuture;
pub use client::tls::ClientTlsOption;
pub use client::Client;
pub use client::ClientBuilder;
pub use client::ClientInterface;
pub use common::sender::SendError;
pub use common::sender::SenderState;
pub use common::window_size::StreamDead;
pub use data_or_trailers::DataOrTrailers;
pub use error::Error;
pub use message::SimpleHttpMessage;
pub use net::addr::AnySocketAddr;
pub use result::Result;
pub use server::conf::ServerAlpn;
pub use server::conf::ServerConf;
pub use server::handler::ServerHandler;
pub use server::handler_paths::ServerHandlerPaths;
pub use server::increase_in_window::ServerIncreaseInWindow;
pub use server::req::ServerRequest;
pub use server::resp::ServerResponse;
pub use server::stream_handler::ServerRequestStreamHandler;
pub use server::tls::ServerTlsOption;
pub use server::Server;
pub use server::ServerBuilder;
pub use solicit::error_code::ErrorCode;
pub use solicit::header::name::HeaderName;
pub use solicit::header::name::PseudoHeaderName;
pub use solicit::header::value::HeaderValue;
pub use solicit::header::Header;
pub use solicit::header::Headers;
pub use solicit::stream_id::StreamId;
pub use solicit::HttpScheme;
pub use stream_after_headers::HttpStreamAfterHeaders;

mod ascii;
mod assert_types;
pub(crate) mod bytes_ext;
mod client;
mod codec;
mod common;
mod data_or_headers;
mod data_or_headers_with_flag;
mod data_or_trailers;
mod death;
mod display_comma_separated;
mod error;
mod futures_misc;
mod headers_place;
mod hpack;
mod log_ndc_future;
mod message;
pub mod misc;
pub(crate) mod net;
mod req_resp;
mod result;
mod server;
mod solicit;
mod solicit_async;
mod solicit_misc;
mod stream_after_headers;

/// Functions used in tests
#[doc(hidden)]
pub mod for_test {
    pub use crate::common::conn::ConnStateSnapshot;
    pub use crate::common::stream::HttpStreamStateSnapshot;
    pub use crate::server::conn::ServerConn;
    pub use crate::solicit::frame::HttpSettings;
    pub use crate::solicit::window_size::WindowSize;
    pub use crate::solicit::DEFAULT_SETTINGS;
    pub use crate::solicit_async::recv_raw_frame_sync;

    pub mod solicit {
        pub use crate::solicit::*;
    }
    pub mod hpack {
        pub use crate::hpack::*;
    }
}
