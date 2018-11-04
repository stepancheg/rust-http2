//! The module contains the implementation of HTTP/2 frames.

use std::mem;

use bytes::Bytes;

use solicit::frame::flags::*;

/// A helper macro that unpacks a sequence of 4 bytes found in the buffer with
/// the given identifier, starting at the given offset, into the given integer
/// type. Obviously, the integer type should be able to support at least 4
/// bytes.
///
/// # Examples
///
/// ```rust
/// let buf: [u8; 4] = [0, 0, 0, 1];
/// assert_eq!(1u32, unpack_octets_4!(buf, 0, u32));
/// ```
macro_rules! unpack_octets_4 {
    ($buf:expr, $offset:expr, $tip:ty) => {
        (($buf[$offset + 0] as $tip) << 24)
            | (($buf[$offset + 1] as $tip) << 16)
            | (($buf[$offset + 2] as $tip) << 8)
            | (($buf[$offset + 3] as $tip) << 0)
    };
}

/// Parse the next 4 octets in the given buffer, assuming they represent an HTTP/2 stream ID.
/// This means that the most significant bit of the first octet is ignored and the rest interpreted
/// as a network-endian 31-bit integer.
#[inline]
fn parse_stream_id(buf: &[u8]) -> u32 {
    let unpacked = unpack_octets_4!(buf, 0, u32);
    // Now clear the most significant bit, as that is reserved and MUST be ignored when received.
    unpacked & !0x80000000
}

pub mod builder;
pub mod continuation;
pub mod data;
pub mod flags;
pub mod goaway;
pub mod headers;
pub mod ping;
pub mod priority;
pub mod push_promise;
pub mod rst_stream;
pub mod settings;
pub mod window_update;

pub use self::builder::FrameBuilder;

pub use self::continuation::ContinuationFrame;
pub use self::data::{DataFlag, DataFrame};
pub use self::goaway::GoawayFrame;
pub use self::headers::{HeadersFlag, HeadersFrame};
pub use self::ping::PingFrame;
pub use self::priority::PriorityFrame;
pub use self::push_promise::PushPromiseFrame;
pub use self::rst_stream::RstStreamFrame;
pub use self::settings::{HttpSetting, SettingsFlag, SettingsFrame};
pub use self::window_update::WindowUpdateFrame;
use codec::write_buffer::WriteBuffer;
use solicit::frame;
use solicit::frame::continuation::CONTINUATION_FRAME_TYPE;
use solicit::frame::data::DATA_FRAME_TYPE;
use solicit::frame::goaway::GOAWAY_FRAME_TYPE;
use solicit::frame::headers::HeadersDecodedFrame;
use solicit::frame::headers::HEADERS_FRAME_TYPE;
use solicit::frame::ping::PING_FRAME_TYPE;
use solicit::frame::priority::PRIORITY_FRAME_TYPE;
use solicit::frame::push_promise::PUSH_PROMISE_FRAME_TYPE;
use solicit::frame::rst_stream::RST_STREAM_FRAME_TYPE;
use solicit::frame::settings::SETTINGS_FRAME_TYPE;
use solicit::frame::window_update::WINDOW_UPDATE_FRAME_TYPE;
use solicit::stream_id::StreamId;
use std::fmt;

pub const FRAME_HEADER_LEN: usize = 9;

/// An alias for the 9-byte buffer that each HTTP/2 frame header must be stored
/// in.
pub type FrameHeaderBuffer = [u8; FRAME_HEADER_LEN];

/// An alias for the 4-tuple representing the components of each HTTP/2 frame
/// header.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct FrameHeader {
    /// payload length
    pub payload_len: u32,
    pub frame_type: u8,
    pub flags: u8,
    pub stream_id: u32,
}

impl FrameHeader {
    pub fn new(payload_len: u32, frame_type: u8, flags: u8, stream_id: u32) -> FrameHeader {
        FrameHeader {
            payload_len,
            frame_type,
            flags,
            stream_id,
        }
    }
}

#[inline]
pub fn unpack_header_from_slice(header: &[u8]) -> FrameHeader {
    assert_eq!(FRAME_HEADER_LEN, header.len());

    let payload_len: u32 =
        ((header[0] as u32) << 16) | ((header[1] as u32) << 8) | (header[2] as u32);
    let frame_type = header[3];
    let flags = header[4];
    let stream_id = parse_stream_id(&header[5..]);

    FrameHeader {
        payload_len,
        frame_type,
        flags,
        stream_id,
    }
}

#[cfg(test)]
pub fn unpack_frames_for_test(mut raw: &[u8]) -> Vec<HttpFrame> {
    let mut r = Vec::new();
    while !raw.is_empty() {
        let raw_frame = RawFrame::parse(raw).unwrap();
        raw = &raw[raw_frame.len()..];
        r.push(HttpFrame::from_raw(&raw_frame).unwrap());
    }
    r
}

/// Deconstructs a `FrameHeader` into its corresponding 4 components,
/// represented as a 4-tuple: `(length, frame_type, flags, stream_id)`.
///
/// The frame `type` and `flags` components are returned as their original
/// octet representation, rather than reinterpreted.
pub fn unpack_header(header: &FrameHeaderBuffer) -> FrameHeader {
    unpack_header_from_slice(header)
}

/// Constructs a buffer of 9 bytes that represents the given `FrameHeader`.
pub fn pack_header(header: &FrameHeader) -> FrameHeaderBuffer {
    let &FrameHeader {
        payload_len,
        frame_type,
        flags,
        stream_id,
    } = header;

    [
        (((payload_len >> 16) & 0x000000FF) as u8),
        (((payload_len >> 8) & 0x000000FF) as u8),
        (((payload_len) & 0x000000FF) as u8),
        frame_type,
        flags,
        (((stream_id >> 24) & 0x000000FF) as u8),
        (((stream_id >> 16) & 0x000000FF) as u8),
        (((stream_id >> 8) & 0x000000FF) as u8),
        (((stream_id) & 0x000000FF) as u8),
    ]
}

/// A helper function that parses the given payload, considering it padded.
///
/// This means that the first byte is the length of the padding with that many
/// 0 bytes expected to follow the actual payload.
///
/// # Returns
///
/// A slice of the given payload where the actual one is found and the length
/// of the padding.
///
/// If the padded payload is invalid (e.g. the length of the padding is equal
/// to the total length), returns `None`.
fn parse_padded_payload(payload: Bytes, flag: bool) -> ParseFrameResult<(Bytes, u8)> {
    if !flag {
        return Ok((payload, 0));
    }
    if payload.len() == 0 {
        // We make sure not to index the payload before we're sure how
        // large the buffer is.
        // If this is the case, the frame is invalid as no padding
        // length can be extracted, even though the frame should be
        // padded.
        return Err(ParseFrameError::ProtocolError);
    }
    let pad_len = payload[0] as usize;
    if pad_len >= payload.len() {
        // This is invalid: the padding length MUST be less than the
        // total frame size.
        return Err(ParseFrameError::ProtocolError);
    }

    Ok((payload.slice(1, payload.len() - pad_len), pad_len as u8))
}

/// A trait that types that are an intermediate representation of HTTP/2 frames should implement.
/// It allows us to generically serialize any intermediate representation into an on-the-wire
/// representation.
pub trait FrameIR: fmt::Debug {
    /// Write out the on-the-wire representation of the frame into the given `FrameBuilder`.
    fn serialize_into(self, builder: &mut WriteBuffer);

    fn serialize_into_vec(self) -> Vec<u8>
    where
        Self: Sized,
    {
        let mut builder = WriteBuffer::new();
        self.serialize_into(&mut builder);
        builder.into()
    }
}

#[derive(Debug)]
pub enum ParseFrameError {
    InternalError,
    BufMustBeAtLeast9Bytes(usize),
    IncorrectPayloadLen,
    StreamIdMustBeNonZero,
    StreamIdMustBeZero(u32),
    StreamDependencyOnItself(u32),
    IncorrectFrameLength(u32),
    IncorrectFlags(u8),
    IncorrectSettingsPushValue(u32),
    IncorrectSettingsMaxFrameSize(u32),
    WindowSizeTooLarge(u32),
    WindowUpdateIncrementInvalid(u32),
    ProtocolError, // generic error
}

pub type ParseFrameResult<T> = Result<T, ParseFrameError>;

/// A trait that all HTTP/2 frame structs need to implement.
pub trait Frame: Sized {
    /// The type that represents the flags that the particular `Frame` can take.
    /// This makes sure that only valid `Flag`s are used with each `Frame`.
    type FlagType: Flag;

    /// Creates a new `Frame` from the given `RawFrame` (i.e. header and
    /// payload), if possible.
    ///
    /// # Returns
    ///
    /// `None` if a valid `Frame` cannot be constructed from the given
    /// `RawFrame`. Some reasons why this may happen is a wrong frame type in
    /// the header, a body that cannot be decoded according to the particular
    /// frame's rules, etc.
    ///
    /// Otherwise, returns a newly constructed `Frame`.
    fn from_raw(raw_frame: &RawFrame) -> ParseFrameResult<Self>;

    /// Frame flags
    fn flags(&self) -> Flags<Self::FlagType>;
    /// Returns the `StreamId` of the stream to which the frame is associated
    fn get_stream_id(&self) -> StreamId;
    /// Returns a `FrameHeader` based on the current state of the `Frame`.
    fn get_header(&self) -> FrameHeader;
}

/// A struct that defines the format of the raw HTTP/2 frame, i.e. the frame
/// as it is read from the wire.
///
/// This format is defined in section 4.1. of the HTTP/2 spec.
///
/// The `RawFrame` struct simply stores the raw components of an HTTP/2 frame:
/// its header and the payload as a sequence of bytes.
///
/// It does not try to interpret the payload bytes, nor do any validation in
/// terms of its validity based on the frame type given in the header.
/// It is simply a wrapper around the two parts of an HTTP/2 frame.
#[derive(PartialEq, Debug, Clone)]
pub struct RawFrame {
    /// The raw frame representation, including both the raw header representation
    /// (in the first 9 bytes), followed by the raw payload representation.
    pub raw_content: Bytes,
}

pub struct RawFrameRef<'a> {
    pub raw_content: &'a [u8],
}

impl RawFrame {
    /// Parses a `RawFrame` from the bytes starting at the beginning of the given buffer.
    ///
    /// Returns the `None` variant when it is not possible to parse a raw frame from the buffer
    /// (due to there not being enough bytes in the buffer). If the `RawFrame` is successfully
    /// parsed it returns the frame, borrowing a part of the original buffer. Therefore, this
    /// method makes no copies, nor does it perform any extra allocations.
    pub fn parse<B: Into<Bytes>>(into_buf: B) -> ParseFrameResult<RawFrame> {
        // TODO(mlalic): This might allow an extra parameter that specifies the maximum frame
        //               payload length?

        let buf = into_buf.into();

        if buf.len() < 9 {
            return Err(ParseFrameError::BufMustBeAtLeast9Bytes(buf.len()));
        }

        // TODO: do not transmute
        let header = unpack_header(unsafe {
            assert!(buf.len() >= 9);
            // We just asserted that this transmute is safe.
            mem::transmute(buf.as_ptr())
        });

        let payload_len = header.payload_len as usize;
        if buf[9..].len() < payload_len {
            return Err(ParseFrameError::IncorrectPayloadLen);
        }

        let raw = &buf[..9 + payload_len];
        Ok(raw.into())
    }

    pub fn as_frame_ref(&self) -> RawFrameRef {
        RawFrameRef {
            raw_content: &self.raw_content,
        }
    }

    pub fn frame_type(&self) -> u8 {
        self.as_frame_ref().frame_type()
    }

    /// Returns the total length of the `RawFrame`, including both headers, as well as the entire
    /// payload.
    #[inline]
    pub fn len(&self) -> usize {
        self.raw_content.len()
    }

    /// Returns a `Vec` of bytes representing the serialized (on-the-wire)
    /// representation of this raw frame.
    pub fn serialize(&self) -> &Bytes {
        &self.raw_content
    }

    /// Returns a `FrameHeader` instance corresponding to the headers of the
    /// `RawFrame`.
    pub fn header(&self) -> FrameHeader {
        unpack_header(unsafe {
            assert!(self.raw_content.len() >= 9);
            // We just asserted that this transmute is safe.
            mem::transmute(self.raw_content.as_ptr())
        })
    }

    pub fn get_stream_id(&self) -> StreamId {
        self.header().stream_id
    }

    /// Returns a slice representing the payload of the `RawFrame`.
    pub fn payload(&self) -> Bytes {
        self.raw_content.slice_from(9)
    }
}

impl<'a> RawFrameRef<'a> {
    pub fn frame_type(&self) -> u8 {
        self.raw_content[3]
    }
}

impl AsRef<[u8]> for RawFrame {
    fn as_ref(&self) -> &[u8] {
        self.raw_content.as_ref()
    }
}
/// Provide a conversion from a `Vec`.
///
/// This conversion is unchecked and could cause the resulting `RawFrame` to be an
/// invalid HTTP/2 frame.
impl From<Vec<u8>> for RawFrame {
    fn from(raw: Vec<u8>) -> RawFrame {
        RawFrame {
            raw_content: Bytes::from(raw),
        }
    }
}
impl<'a> From<&'a [u8]> for RawFrame {
    fn from(raw: &'a [u8]) -> RawFrame {
        RawFrame {
            raw_content: Bytes::from(raw),
        }
    }
}

/// `RawFrame`s can be serialized to an on-the-wire format.
impl FrameIR for RawFrame {
    fn serialize_into(self, b: &mut WriteBuffer) {
        b.write_header(self.header());
        b.extend_from_bytes(self.payload());
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_header, unpack_header, FrameHeader, RawFrame};

    /// Tests that the `unpack_header` function correctly returns the
    /// components of HTTP/2 frame headers.
    #[test]
    fn test_unpack_header() {
        {
            let header = [0, 0, 1, 2, 3, 0, 0, 0, 4];
            assert_eq!(
                FrameHeader {
                    payload_len: 1,
                    frame_type: 2,
                    flags: 3,
                    stream_id: 4
                },
                unpack_header(&header)
            );
        }
        {
            let header = [0, 1, 0, 0, 0, 0, 0, 0, 0];
            assert_eq!(
                FrameHeader {
                    payload_len: 256,
                    frame_type: 0,
                    flags: 0,
                    stream_id: 0
                },
                unpack_header(&header)
            );
        }
        {
            let header = [1, 0, 0, 0, 0, 0, 0, 0, 0];
            assert_eq!(
                FrameHeader {
                    payload_len: 256 * 256,
                    frame_type: 0,
                    flags: 0,
                    stream_id: 0
                },
                unpack_header(&header)
            );
        }
        {
            let header = [0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 1];
            assert_eq!(
                FrameHeader {
                    payload_len: (1 << 24) - 1,
                    frame_type: 0,
                    flags: 0,
                    stream_id: 1
                },
                unpack_header(&header)
            );
        }
        {
            let header = [0xFF, 0xFF, 0xFF, 0, 0, 1, 1, 1, 1];
            assert_eq!(
                FrameHeader {
                    payload_len: (1 << 24) - 1,
                    frame_type: 0,
                    flags: 0,
                    stream_id: 1 + (1 << 8) + (1 << 16) + (1 << 24)
                },
                unpack_header(&header)
            );
        }
        {
            // Ignores reserved bit within the stream id (the most significant bit)
            let header = [0, 0, 1, 0, 0, 0x80, 0, 0, 1];
            assert_eq!(
                FrameHeader {
                    payload_len: 1,
                    frame_type: 0,
                    flags: 0,
                    stream_id: 1
                },
                unpack_header(&header)
            );
        }
    }

    /// Tests that the `pack_header` function correctly returns the buffer
    /// corresponding to components of HTTP/2 frame headers.
    #[test]
    fn test_pack_header() {
        {
            let header = [0; 9];
            assert_eq!(pack_header(&FrameHeader::new(0, 0, 0, 0)), header);
        }
        {
            let header = [0, 0, 1, 2, 3, 0, 0, 0, 4];
            assert_eq!(pack_header(&FrameHeader::new(1, 2, 3, 4)), header);
        }
        {
            let header = [0, 0, 1, 200, 100, 0, 0, 0, 4];
            assert_eq!(pack_header(&FrameHeader::new(1, 200, 100, 4)), header);
        }
        {
            let header = [0, 0, 1, 0, 0, 0, 0, 0, 0];
            assert_eq!(pack_header(&FrameHeader::new(1, 0, 0, 0)), header);
        }
        {
            let header = [0, 1, 0, 0, 0, 0, 0, 0, 0];
            assert_eq!(pack_header(&FrameHeader::new(256, 0, 0, 0)), header);
        }
        {
            let header = [1, 0, 0, 0, 0, 0, 0, 0, 0];
            assert_eq!(pack_header(&FrameHeader::new(256 * 256, 0, 0, 0)), header);
        }
        {
            let header = [0, 0, 0, 0, 0, 0, 0, 0, 1];
            assert_eq!(pack_header(&FrameHeader::new(0, 0, 0, 1)), header);
        }
        {
            let header = [0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 1];
            assert_eq!(
                pack_header(&FrameHeader::new((1 << 24) - 1, 0, 0, 1)),
                header
            );
        }
        {
            let header = [0xFF, 0xFF, 0xFF, 0, 0, 1, 1, 1, 1];
            let header_components =
                FrameHeader::new((1 << 24) - 1, 0, 0, 1 + (1 << 8) + (1 << 16) + (1 << 24));
            assert_eq!(pack_header(&header_components), header);
        }
    }

    /// Builds a `Vec` containing the given data as a padded HTTP/2 frame.
    ///
    /// It first places the length of the padding, followed by the data,
    /// followed by `pad_len` zero bytes.
    pub fn build_padded_frame_payload(data: &[u8], pad_len: u8) -> Vec<u8> {
        let sz = 1 + data.len() + pad_len as usize;
        let mut payload: Vec<u8> = Vec::with_capacity(sz);
        payload.push(pad_len);
        payload.extend(data.to_vec().into_iter());
        for _ in 0..pad_len {
            payload.push(0);
        }

        payload
    }

    /// Tests that a borrowed slice can be converted into a `RawFrame` due to the implementation of
    /// the `From<&'a [u8]>` trait.
    #[test]
    fn test_from_slice() {
        let buf = &b""[..];
        let frame = RawFrame::from(buf);
        assert_eq!(frame.as_ref(), buf);
    }

    /// Tests that the `RawFrame::serialize` method correctly serializes a
    /// `RawFrame`.
    #[test]
    fn test_raw_frame_serialize() {
        let data = b"123";
        let header = FrameHeader {
            payload_len: data.len() as u32,
            frame_type: 0x1,
            flags: 0,
            stream_id: 1,
        };
        let buf = {
            let mut buf = Vec::new();
            buf.extend(pack_header(&header).to_vec().into_iter());
            buf.extend(data.to_vec().into_iter());
            buf
        };
        let raw: RawFrame = buf.clone().into();

        assert_eq!(raw.serialize().as_ref(), &buf[..]);
    }

    /// Tests the `len` method of the `RawFrame`.
    #[test]
    fn test_raw_frame_len() {
        {
            // Not enough bytes for even the header of the frame
            let buf = b"123";
            let frame = RawFrame::from(&buf[..]);
            assert_eq!(buf.len(), frame.len());
        }
        {
            // Full header, but not enough bytes for the payload
            let buf = vec![0, 0, 1, 0, 0, 0, 0, 0, 0];
            let frame = RawFrame::from(&buf[..]);
            assert_eq!(buf.len(), frame.len());
        }
        {
            let buf = vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 1];
            let frame = RawFrame::from(&buf[..]);
            assert_eq!(buf.len(), frame.len());
        }
    }
}

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

impl HttpFrameType {
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
            HttpFrameType::Unknown(t) => *t,
        }
    }
}

/// An enum representing all frame variants that can be returned by an `HttpConnection` can handle.
///
/// The variants wrap the appropriate `Frame` implementation, except for the `UnknownFrame`
/// variant, which provides an owned representation of the underlying `RawFrame`
#[derive(PartialEq, Debug, Clone)]
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

impl HttpFrame {
    // TODO: take by value
    pub fn from_raw(raw_frame: &RawFrame) -> ParseFrameResult<HttpFrame> {
        let frame = match raw_frame.header().frame_type {
            frame::data::DATA_FRAME_TYPE => HttpFrame::Data(HttpFrame::parse_frame(&raw_frame)?),
            frame::headers::HEADERS_FRAME_TYPE => {
                HttpFrame::Headers(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::priority::PRIORITY_FRAME_TYPE => {
                HttpFrame::Priority(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::rst_stream::RST_STREAM_FRAME_TYPE => {
                HttpFrame::RstStream(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::settings::SETTINGS_FRAME_TYPE => {
                HttpFrame::Settings(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::push_promise::PUSH_PROMISE_FRAME_TYPE => {
                HttpFrame::PushPromise(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::ping::PING_FRAME_TYPE => HttpFrame::Ping(HttpFrame::parse_frame(&raw_frame)?),
            frame::goaway::GOAWAY_FRAME_TYPE => {
                HttpFrame::Goaway(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::window_update::WINDOW_UPDATE_FRAME_TYPE => {
                HttpFrame::WindowUpdate(HttpFrame::parse_frame(&raw_frame)?)
            }
            frame::continuation::CONTINUATION_FRAME_TYPE => {
                HttpFrame::Continuation(HttpFrame::parse_frame(&raw_frame)?)
            }
            _ => HttpFrame::Unknown(raw_frame.as_ref().into()),
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
    fn parse_frame<F: Frame>(raw_frame: &RawFrame) -> ParseFrameResult<F> {
        Frame::from_raw(&raw_frame)
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
    fn serialize_into(self, builder: &mut WriteBuffer) {
        match self {
            HttpFrame::Data(f) => f.serialize_into(builder),
            HttpFrame::Headers(f) => f.serialize_into(builder),
            HttpFrame::Priority(f) => f.serialize_into(builder),
            HttpFrame::RstStream(f) => f.serialize_into(builder),
            HttpFrame::Settings(f) => f.serialize_into(builder),
            HttpFrame::PushPromise(f) => f.serialize_into(builder),
            HttpFrame::Ping(f) => f.serialize_into(builder),
            HttpFrame::Goaway(f) => f.serialize_into(builder),
            HttpFrame::WindowUpdate(f) => f.serialize_into(builder),
            HttpFrame::Continuation(f) => f.serialize_into(builder),
            HttpFrame::Unknown(f) => f.serialize_into(builder),
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

#[derive(Debug, Clone)]
pub enum HttpFrameDecoded {
    Data(DataFrame),
    Headers(HeadersDecodedFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    Settings(SettingsFrame),
    PushPromise(PushPromiseFrame),
    Ping(PingFrame),
    Goaway(GoawayFrame),
    WindowUpdate(WindowUpdateFrame),
    Unknown(RawFrame),
}
