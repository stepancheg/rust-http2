//! The module implements the framing layer of HTTP/2 and exposes an API for using it.

use std::fmt;
use std::u32;

use self::frame::settings::HttpSettings;

pub mod end_stream;
pub(crate) mod error_code;
pub mod frame;
pub mod header;
pub mod session;
pub(crate) mod stream_id;

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

/// A sender MUST NOT allow a flow-control window to exceed 231-1 octets. If a sender receives
/// a WINDOW_UPDATE that causes a flow-control window to exceed this maximum,
/// it MUST terminate either the stream or the connection, as appropriate. For streams,
/// the sender sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the connection,
/// a GOAWAY frame with an error code of FLOW_CONTROL_ERROR is sent.
pub const MAX_WINDOW_SIZE: u32 = 0x7fffffff;

// 6.9 WINDOW_UPDATE
/// The payload of a WINDOW_UPDATE frame is one reserved bit plus an unsigned 31-bit integer
/// indicating the number of octets that the sender can transmit in addition to the existing
/// flow-control window. The legal range for the increment to the flow-control window
/// is 1 to 231-1 (2,147,483,647) octets.
pub const MAX_WINDOW_SIZE_INC: u32 = 0x7fffffff;

/// The struct represents the size of a flow control window.
///
/// It exposes methods that allow the manipulation of window sizes, such that they can never
/// overflow the spec-mandated upper bound.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WindowSize(pub i32);
impl WindowSize {
    /// Tries to increase the window size by the given delta. If the WindowSize would overflow the
    /// maximum allowed value (2^31 - 1), returns an error case. If the increase succeeds, returns
    /// `Ok`.
    pub fn try_increase(&mut self, delta: u32) -> Result<(), ()> {
        // Someone's provided a delta that would definitely overflow the window size.
        if delta > MAX_WINDOW_SIZE_INC || delta == 0 {
            return Err(());
        }
        // Now it is safe to cast the delta to the `i32`.
        match self.0.checked_add(delta as i32) {
            None => {
                // When the add overflows, we will have went over the maximum allowed size of the
                // window size...
                Err(())
            }
            Some(next_val) => {
                // The addition didn't overflow, so the next window size is in the range allowed by
                // the spec.
                self.0 = next_val;
                Ok(())
            }
        }
    }

    /// Tries to decrease the size of the window by the given delta.
    ///
    /// There are situations where the window size should legitimately be allowed to become
    /// negative, so the only situation where the result is an error is if the window size would
    /// underflow, as this would definitely cause the peers to lose sync.
    pub fn try_decrease(&mut self, delta: i32) -> Result<(), ()> {
        match self.0.checked_sub(delta) {
            Some(new) => {
                self.0 = new;
                Ok(())
            }
            None => Err(()),
        }
    }

    pub fn try_decrease_to_positive(&mut self, delta: i32) -> Result<(), ()> {
        match self.0.checked_sub(delta) {
            Some(new) if new >= 0 => {
                self.0 = new;
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Creates a new `WindowSize` with the given initial size.
    pub fn new(size: i32) -> WindowSize {
        WindowSize(size)
    }
    /// Returns the current size of the window.
    ///
    /// The size is actually allowed to become negative (for instance if the peer changes its
    /// intial window size in the settings); therefore, the return is an `i32`.
    pub fn size(&self) -> i32 {
        self.0
    }

    /// Window size when it's know to be non-negative
    ///
    /// Panics if windows size if negative
    pub fn unsigned(&self) -> u32 {
        let r = self.0 as u32;
        assert_eq!(r as i32, self.0);
        r
    }
}

impl fmt::Display for WindowSize {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

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
