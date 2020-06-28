//! The module implements the framing layer of HTTP/2 and exposes an API for using it.

use crate::solicit::frame::HttpSettings;
use std::u32;

pub(crate) mod end_stream;
pub(crate) mod error_code;
pub mod frame;
pub mod header;
pub mod session;
pub(crate) mod stream_id;
pub(crate) mod window_size;

/// Default settings.
// 6.5.2 Defined SETTINGS Parameters
pub const DEFAULT_SETTINGS: HttpSettings = HttpSettings {
    header_table_size: 4_096,
    enable_push: true,
    max_concurrent_streams: u32::MAX,
    initial_window_size: 65_535,
    max_frame_size: 16_384,
    max_header_list_size: u32::MAX,
};

/// A set of protocol names that the library should use to indicate that HTTP/2
/// is supported during protocol negotiation (NPN or ALPN).
/// We include some of the drafts' protocol names, since there is basically no
/// difference for all intents and purposes (and some servers out there still
/// only officially advertise draft support).
/// TODO: Eventually only use "h2".
pub const ALPN_PROTOCOLS: &'static [&'static [u8]] = &[b"h2", b"h2-16", b"h2-15", b"h2-14"];

/// An enum representing the two possible HTTP schemes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum HttpScheme {
    /// The variant corresponding to `http://`
    Http,
    /// The variant corresponding to `https://`
    Https,
}

impl HttpScheme {
    /// Returns a byte string representing the scheme.
    #[inline]
    pub fn as_bytes(&self) -> &'static [u8] {
        match *self {
            HttpScheme::Http => b"http",
            HttpScheme::Https => b"https",
        }
    }
}

#[cfg(test)]
pub mod tests;
