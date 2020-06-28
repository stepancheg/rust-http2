//! HTTP/2 frame type utilities.

use crate::solicit::frame::continuation::CONTINUATION_FRAME_TYPE;
use crate::solicit::frame::data::DATA_FRAME_TYPE;
use crate::solicit::frame::goaway::GOAWAY_FRAME_TYPE;
use crate::solicit::frame::headers::HEADERS_FRAME_TYPE;
use crate::solicit::frame::ping::PING_FRAME_TYPE;
use crate::solicit::frame::priority::PRIORITY_FRAME_TYPE;
use crate::solicit::frame::push_promise::PUSH_PROMISE_FRAME_TYPE;
use crate::solicit::frame::rst_stream::RST_STREAM_FRAME_TYPE;
use crate::solicit::frame::settings::SETTINGS_FRAME_TYPE;
use crate::solicit::frame::window_update::WINDOW_UPDATE_FRAME_TYPE;
use std::fmt;

/// All known frame types.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum HttpFrameType {
    /// `DATA`
    Data,
    /// `HEADERS`
    Headers,
    /// `PRIORITY`
    Priority,
    /// `RST_STREAM`
    RstStream,
    /// `SETTINGS`
    Settings,
    /// `PUSH_PROMISE`
    PushPromise,
    /// `PING`
    Ping,
    /// `GOAWAY`
    Goaway,
    /// `WINDOW_UPDATE`
    WindowUpdate,
    /// `CONTINUATION`
    Continuation,
}

impl HttpFrameType {
    /// All known frame types
    const ALL: &'static [HttpFrameType] = &[
        HttpFrameType::Data,
        HttpFrameType::Headers,
        HttpFrameType::Priority,
        HttpFrameType::RstStream,
        HttpFrameType::Settings,
        HttpFrameType::PushPromise,
        HttpFrameType::Ping,
        HttpFrameType::Goaway,
        HttpFrameType::WindowUpdate,
        HttpFrameType::Continuation,
    ];
}

/// HTTP frame type id.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct RawHttpFrameType(pub u8);

impl RawHttpFrameType {
    pub const DATA: RawHttpFrameType = RawHttpFrameType(DATA_FRAME_TYPE);
    pub const HEADERS: RawHttpFrameType = RawHttpFrameType(HEADERS_FRAME_TYPE);
    pub const PRIORITY: RawHttpFrameType = RawHttpFrameType(PRIORITY_FRAME_TYPE);
    pub const RST_STREAM: RawHttpFrameType = RawHttpFrameType(RST_STREAM_FRAME_TYPE);
    pub const SETTINGS: RawHttpFrameType = RawHttpFrameType(SETTINGS_FRAME_TYPE);
    pub const PUSH_PROMISE: RawHttpFrameType = RawHttpFrameType(PUSH_PROMISE_FRAME_TYPE);
    pub const PING: RawHttpFrameType = RawHttpFrameType(PING_FRAME_TYPE);
    pub const GOAWAY: RawHttpFrameType = RawHttpFrameType(GOAWAY_FRAME_TYPE);
    pub const WINDOW_UPDATE: RawHttpFrameType = RawHttpFrameType(WINDOW_UPDATE_FRAME_TYPE);
    pub const CONTINUATION: RawHttpFrameType = RawHttpFrameType(CONTINUATION_FRAME_TYPE);

    fn known(&self) -> Result<HttpFrameType, u8> {
        HttpFrameType::ALL
            .iter()
            .find(|t| t.frame_type() == self.0)
            .cloned()
            .ok_or(self.0)
    }
}

impl HttpFrameType {
    /// Frame type as byte.
    pub fn frame_type(&self) -> u8 {
        match self {
            HttpFrameType::Data => DATA_FRAME_TYPE,
            HttpFrameType::Headers => HEADERS_FRAME_TYPE,
            HttpFrameType::Priority => PRIORITY_FRAME_TYPE,
            HttpFrameType::RstStream => RST_STREAM_FRAME_TYPE,
            HttpFrameType::Settings => SETTINGS_FRAME_TYPE,
            HttpFrameType::PushPromise => PUSH_PROMISE_FRAME_TYPE,
            HttpFrameType::Ping => PING_FRAME_TYPE,
            HttpFrameType::Goaway => GOAWAY_FRAME_TYPE,
            HttpFrameType::WindowUpdate => WINDOW_UPDATE_FRAME_TYPE,
            HttpFrameType::Continuation => CONTINUATION_FRAME_TYPE,
        }
    }
}

impl fmt::Display for HttpFrameType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpFrameType::Data => write!(f, "DATA"),
            HttpFrameType::Headers => write!(f, "HEADERS"),
            HttpFrameType::Priority => write!(f, "PRIORITY"),
            HttpFrameType::RstStream => write!(f, "RST_STREAM"),
            HttpFrameType::Settings => write!(f, "SETTINGS"),
            HttpFrameType::PushPromise => write!(f, "PUSH_PROMISE"),
            HttpFrameType::Ping => write!(f, "PING"),
            HttpFrameType::Goaway => write!(f, "GOAWAY"),
            HttpFrameType::WindowUpdate => write!(f, "WINDOW_UPDATE"),
            HttpFrameType::Continuation => write!(f, "CONTINUATION"),
        }
    }
}

impl fmt::Display for RawHttpFrameType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.known() {
            Ok(k) => fmt::Display::fmt(&k, f),
            Err(t) => write!(f, "UNKNOWN({})", t),
        }
    }
}
