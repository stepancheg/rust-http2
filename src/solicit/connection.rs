//! The module contains the implementation of an HTTP/2 connection.
//!
//! This provides an API to read and write raw HTTP/2 frames, as well as a way to hook into
//! higher-level events arising on an HTTP/2 connection, such as the receipt of headers on a
//! particular stream or a new data chunk.
//!
//! The `SendFrame` and `ReceiveFrame` traits are the API to sending and receiving frames off of an
//! HTTP/2 connection. The module includes default implementations of those traits for `io::Write`
//! and `solicit::http::transport::TransportStream` types.
//!
//! The `HttpConnection` struct builds on top of these traits and provides an API for sending
//! messages of a higher level to the peer (such as writing data or headers, while automatically
//! handling the framing and header encoding), as well as for handling incoming events of that
//! type. The `Session` trait is the bridge between the connection layer (i.e. the
//! `HttpConnection`) and the higher layers that handle these events and pass them on to the
//! application.

use std::borrow::Cow;
use std::borrow::Borrow;

use error::Error;
use result::Result;
use solicit::{StreamId, WindowSize};
use solicit::DEFAULT_SETTINGS;
use solicit::frame;
use solicit::frame::*;
use solicit::frame::settings::HttpSettings;
use hpack;

#[derive(Debug, PartialEq, Eq)]
pub enum HttpFrameType {
    Data,
    Headers,
    Priority,
    RstStream,
    Settings,
    PushPromise,
    Ping,
    Goaway,
    WindowUpdate,
    Continuation,
    Unknown(u8),
}

/// An enum representing all frame variants that can be returned by an `HttpConnection` can handle.
///
/// The variants wrap the appropriate `Frame` implementation, except for the `UnknownFrame`
/// variant, which provides an owned representation of the underlying `RawFrame`
#[derive(PartialEq)]
#[derive(Debug)]
#[derive(Clone)]
pub enum HttpFrame {
    Data(DataFrame),
    Headers(HeadersFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    Settings(SettingsFrame),
    PushPromise(PushPromiseFrame),
    Ping(PingFrame),
    Goaway(GoawayFrame),
    WindowUpdate(WindowUpdateFrame),
    Continuation(ContinuationFrame),
    Unknown(RawFrame),
}

impl HttpFrame{
    pub fn from_raw(raw_frame: &RawFrame) -> Result<HttpFrame> {
        let frame = match raw_frame.header().frame_type {
            frame::data::DATA_FRAME_TYPE =>
                HttpFrame::Data(HttpFrame::parse_frame(&raw_frame)?),
            frame::headers::HEADERS_FRAME_TYPE =>
                HttpFrame::Headers(HttpFrame::parse_frame(&raw_frame)?),
            frame::priority::PRIORITY_FRAME_TYPE =>
                HttpFrame::Priority(HttpFrame::parse_frame(&raw_frame)?),
            frame::rst_stream::RST_STREAM_FRAME_TYPE =>
                HttpFrame::RstStream(HttpFrame::parse_frame(&raw_frame)?),
            frame::settings::SETTINGS_FRAME_TYPE =>
                HttpFrame::Settings(HttpFrame::parse_frame(&raw_frame)?),
            frame::push_promise::PUSH_PROMISE_FRAME_TYPE =>
                HttpFrame::PushPromise(HttpFrame::parse_frame(&raw_frame)?),
            frame::ping::PING_FRAME_TYPE =>
                HttpFrame::Ping(HttpFrame::parse_frame(&raw_frame)?),
            frame::goaway::GOAWAY_FRAME_TYPE =>
                HttpFrame::Goaway(HttpFrame::parse_frame(&raw_frame)?),
            frame::window_update::WINDOW_UPDATE_FRAME_TYPE =>
                HttpFrame::WindowUpdate(HttpFrame::parse_frame(&raw_frame)?),
            frame::continuation::CONTINUATION_FRAME_TYPE =>
                HttpFrame::Continuation(HttpFrame::parse_frame(&raw_frame)?),
            _ =>
                HttpFrame::Unknown(raw_frame.as_ref().into()),
        };

        Ok(frame)
    }

    /// A helper method that parses the given `RawFrame` into the given `Frame`
    /// implementation.
    ///
    /// # Returns
    ///
    /// Failing to decode the given `Frame` from the `raw_frame`, an
    /// `HttpError::InvalidFrame` error is returned.
    #[inline] // TODO: take by value
    fn parse_frame<F: Frame>(raw_frame: &RawFrame) -> Result<F> {
        // TODO: The reason behind being unable to decode the frame should be
        //       extracted to allow an appropriate connection-level action to be
        //       taken (e.g. responding with a PROTOCOL_ERROR).
        match Frame::from_raw(&raw_frame) {
            Some(f) => Ok(f),
            None => Err(Error::InvalidFrame(
                format!("failed to parse frame {:?}", raw_frame.header()))),
        }
    }

    /// Get stream id, zero for special frames
    pub fn get_stream_id(&self) -> StreamId {
        match self {
            &HttpFrame::Data(ref f) => f.get_stream_id(),
            &HttpFrame::Headers(ref f) => f.get_stream_id(),
            &HttpFrame::Priority(ref f) => f.get_stream_id(),
            &HttpFrame::RstStream(ref f) => f.get_stream_id(),
            &HttpFrame::Settings(ref f) => f.get_stream_id(),
            &HttpFrame::PushPromise(ref f) => f.get_stream_id(),
            &HttpFrame::Ping(ref f) => f.get_stream_id(),
            &HttpFrame::Goaway(ref f) => f.get_stream_id(),
            &HttpFrame::WindowUpdate(ref f) => f.get_stream_id(),
            &HttpFrame::Continuation(ref f) => f.get_stream_id(),
            &HttpFrame::Unknown(ref f) => f.get_stream_id(),
        }
    }

    pub fn frame_type(&self) -> HttpFrameType {
        match self {
            &HttpFrame::Data(..) => HttpFrameType::Data,
            &HttpFrame::Headers(..) => HttpFrameType::Headers,
            &HttpFrame::Priority(..) => HttpFrameType::Priority,
            &HttpFrame::RstStream(..) => HttpFrameType::RstStream,
            &HttpFrame::Settings(..) => HttpFrameType::Settings,
            &HttpFrame::PushPromise(..) => HttpFrameType::PushPromise,
            &HttpFrame::Ping(..) => HttpFrameType::Ping,
            &HttpFrame::Goaway(..) => HttpFrameType::Goaway,
            &HttpFrame::WindowUpdate(..) => HttpFrameType::WindowUpdate,
            &HttpFrame::Continuation(..) => HttpFrameType::Continuation,
            &HttpFrame::Unknown(ref f) => HttpFrameType::Unknown(f.frame_type()),
        }
    }
}

impl FrameIR for HttpFrame {
    fn serialize_into(self, builder: &mut FrameBuilder) {
        match self {
            HttpFrame::Data(f)         => f.serialize_into(builder),
            HttpFrame::Headers(f)      => f.serialize_into(builder),
            HttpFrame::Priority(f)     => f.serialize_into(builder),
            HttpFrame::RstStream(f)    => f.serialize_into(builder),
            HttpFrame::Settings(f)     => f.serialize_into(builder),
            HttpFrame::PushPromise(f)  => f.serialize_into(builder),
            HttpFrame::Ping(f)         => f.serialize_into(builder),
            HttpFrame::Goaway(f)       => f.serialize_into(builder),
            HttpFrame::WindowUpdate(f) => f.serialize_into(builder),
            HttpFrame::Continuation(f) => f.serialize_into(builder),
            HttpFrame::Unknown(f)      => f.serialize_into(builder),
        }
    }
}

impl From<DataFrame> for HttpFrame {
    fn from(frame: DataFrame) -> Self {
        HttpFrame::Data(frame)
    }
}

impl From<HeadersFrame> for HttpFrame {
    fn from(frame: HeadersFrame) -> Self {
        HttpFrame::Headers(frame)
    }
}

impl From<PriorityFrame> for HttpFrame {
    fn from(frame: PriorityFrame) -> Self {
        HttpFrame::Priority(frame)
    }
}

impl From<RstStreamFrame> for HttpFrame {
    fn from(frame: RstStreamFrame) -> Self {
        HttpFrame::RstStream(frame)
    }
}

impl From<SettingsFrame> for HttpFrame {
    fn from(frame: SettingsFrame) -> Self {
        HttpFrame::Settings(frame)
    }
}

impl From<PushPromiseFrame> for HttpFrame {
    fn from(frame: PushPromiseFrame) -> Self {
        HttpFrame::PushPromise(frame)
    }
}

impl From<PingFrame> for HttpFrame {
    fn from(frame: PingFrame) -> Self {
        HttpFrame::Ping(frame)
    }
}

impl From<GoawayFrame> for HttpFrame {
    fn from(frame: GoawayFrame) -> Self {
        HttpFrame::Goaway(frame)
    }
}

impl From<WindowUpdateFrame> for HttpFrame {
    fn from(frame: WindowUpdateFrame) -> Self {
        HttpFrame::WindowUpdate(frame)
    }
}

impl From<ContinuationFrame> for HttpFrame {
    fn from(frame: ContinuationFrame) -> Self {
        HttpFrame::Continuation(frame)
    }
}

/// The struct implements the HTTP/2 connection level logic.
///
/// This means that the struct is a bridge between the low level raw frame reads/writes (i.e. what
/// the `SendFrame` and `ReceiveFrame` traits do) and the higher session-level logic.
///
/// Therefore, it provides an API that exposes higher-level write operations, such as writing
/// headers or data, that take care of all the underlying frame construction that is required.
///
/// Similarly, it provides an API for handling events that arise from receiving frames, without
/// requiring the higher level to directly look at the frames themselves, rather only the semantic
/// content within the frames.
pub struct HttpConnection {
    /// HPACK decoder used to decode incoming headers before passing them on to the session.
    pub decoder: hpack::Decoder<'static>,
    /// The HPACK encoder used to encode headers before sending them on this connection.
    pub encoder: hpack::Encoder<'static>,
    /// Tracks the size of the outbound flow control window
    pub out_window_size: WindowSize,
    /// Tracks the size of the inbound flow control window
    pub in_window_size: WindowSize,
    /// Last known peer settings
    pub peer_settings: HttpSettings,
    /// Last our settings acknowledged
    pub our_settings_ack: HttpSettings,
    /// Last our settings sent
    pub our_settings_sent: Option<HttpSettings>,
}

impl HttpConnection {
    pub fn our_settings_effective(&self) -> HttpSettings {
        if let Some(ref sent) = self.our_settings_sent {
            HttpSettings::min(&self.our_settings_ack, &sent)
        } else {
            self.our_settings_ack
        }
    }
}

/// The struct represents a chunk of data that should be sent to the peer on a particular stream.
pub struct DataChunk<'a> {
    /// The data that should be sent.
    pub data: Cow<'a, [u8]>,
    /// The ID of the stream on which the data should be sent.
    pub stream_id: StreamId,
    /// Whether the data chunk will also end the stream.
    pub end_stream: EndStream,
}

impl<'a> DataChunk<'a> {
    /// Creates a new `DataChunk`.
    ///
    /// **Note:** `IntoCow` is unstable and there's no implementation of `Into<Cow<'a, [u8]>>` for
    /// the fundamental types, making this a bit of a clunky API. Once such an `Into` impl is
    /// added, this can be made generic over the trait for some ergonomic improvements.
    pub fn new(data: Cow<'a, [u8]>, stream_id: StreamId, end_stream: EndStream) -> DataChunk<'a> {
        DataChunk {
            data: data,
            stream_id: stream_id,
            end_stream: end_stream,
        }
    }

    /// Creates a new `DataChunk` from a borrowed slice. This method should become obsolete if we
    /// can take an `Into<Cow<_, _>>` without using unstable features.
    pub fn new_borrowed<D: Borrow<&'a [u8]>>(data: D,
                                             stream_id: StreamId,
                                             end_stream: EndStream)
                                             -> DataChunk<'a> {
        DataChunk {
            data: Cow::Borrowed(data.borrow()),
            stream_id: stream_id,
            end_stream: end_stream,
        }
    }
}

/// An enum indicating whether the `HttpConnection` send operation should end the stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EndStream {
    /// The stream should be closed
    Yes,
    /// The stream should still be kept open
    No,
}

impl HttpConnection {
    /// Creates a new `HttpConnection` that will use the given sender
    /// for writing frames.
    pub fn new() -> HttpConnection {
        HttpConnection {
            decoder: hpack::Decoder::new(),
            encoder: hpack::Encoder::new(),
            peer_settings: DEFAULT_SETTINGS,
            our_settings_ack: DEFAULT_SETTINGS,
            our_settings_sent: None,
            in_window_size: WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32),
            out_window_size: WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32),
        }
    }

    /// Internal helper method that decreases the outbound flow control window size.
    pub fn decrease_out_window(&mut self, size: u32) -> Result<()> {
        // The size by which we decrease the window must be at most 2^31 - 1. We should be able to
        // reach here only after sending a DATA frame, whose payload also cannot be larger than
        // that, but we assert it just in case.
        debug_assert!(size < 0x80000000);
        self.out_window_size
            .try_decrease(size as i32)
            .map_err(|_| Error::WindowSizeOverflow)
    }

    /// Internal helper method that decreases the inbound flow control window size.
    pub fn decrease_in_window(&mut self, size: u32) -> Result<()> {
        // The size by which we decrease the window must be at most 2^31 - 1. We should be able to
        // reach here only after receiving a DATA frame, which would have been validated when
        // parsed from the raw frame to have the correct payload size, but we assert it just in
        // case.
        debug_assert!(size < 0x80000000);
        self.in_window_size
            .try_decrease_to_positive(size as i32)
            .map_err(|_| Error::WindowSizeOverflow)
    }
}
