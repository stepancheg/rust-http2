use std::io;

use bytes::Bytes;

use solicit::StreamId;
use solicit::frame::RawFrame;
use solicit::frame::Frame;
use solicit::frame::FrameIR;
use solicit::frame::FrameHeader;
use solicit::frame::builder::FrameBuilder;

use super::flags::Flag;
use super::flags::Flags;


pub const CONTINUATION_FRAME_TYPE: u8 = 0x9;

/// An enum representing the flags that a `ContinuationFrame` can have.
#[derive(Clone)]
#[derive(PartialEq)]
#[derive(Debug)]
#[derive(Copy)]
pub enum ContinuationFlag {
    EndHeaders = 0x4,
}

impl Flag for ContinuationFlag {
    #[inline]
    fn bitmask(&self) -> u8 {
        *self as u8
    }

    fn flags() -> &'static [Self] {
        static FLAGS: &'static [ContinuationFlag] = &[
            ContinuationFlag::EndHeaders,
        ];
        FLAGS
    }
}

/// The CONTINUATION frame (type=0x9) is used to continue a sequence of header block fragments
/// (Section 4.3). Any number of CONTINUATION frames can be sent, as long as the preceding
/// frame is on the same stream and is a HEADERS, PUSH_PROMISE, or CONTINUATION frame without
/// the END_HEADERS flag set.
///
/// https://http2.github.io/http2-spec/#CONTINUATION
#[derive(PartialEq, Clone, Debug)]
pub struct ContinuationFrame {
    /// The set of flags for the frame, packed into a single byte.
    flags: Flags<ContinuationFlag>,
    /// The ID of the stream with which this frame is associated
    pub stream_id: StreamId,
    /// The header fragment bytes stored within the frame.
    pub header_fragment: Bytes,
}

impl ContinuationFrame {
    pub fn new<B : Into<Bytes>>(fragment: B, stream_id: StreamId) -> ContinuationFrame {
        ContinuationFrame {
            header_fragment: fragment.into(),
            stream_id: stream_id,
            flags: Flags::default(),
        }
    }

    /// Returns the length of the payload of the current frame, including any
    /// possible padding in the number of bytes.
    fn payload_len(&self) -> u32 {
        self.header_fragment.len() as u32
    }

    /// Returns whether this frame ends the headers. If not, there MUST be a
    /// number of follow up CONTINUATION frames that send the rest of the
    /// header data.
    pub fn is_headers_end(&self) -> bool {
        self.flags.is_set(ContinuationFlag::EndHeaders)
    }

    /// Sets the given flag for the frame.
    pub fn set_flag(&mut self, flag: ContinuationFlag) {
        self.flags.set(&flag);
    }
}

impl Frame for ContinuationFrame {
    type FlagType = ContinuationFlag;

    fn from_raw(raw_frame: &RawFrame) -> Option<ContinuationFrame> {
        // Unpack the header
        let FrameHeader { length, frame_type, flags, stream_id } = raw_frame.header();
        // Check that the frame type is correct for this frame implementation
        if frame_type != CONTINUATION_FRAME_TYPE {
            return None;
        }
        // Check that the length given in the header matches the payload
        // length; if not, something went wrong and we do not consider this a
        // valid frame.
        if (length as usize) != raw_frame.payload().len() {
            return None;
        }
        // Check that the HEADERS frame is not associated to stream 0
        if stream_id == 0 {
            return None;
        }

        Some(ContinuationFrame {
            header_fragment: raw_frame.payload(),
            stream_id: stream_id,
            flags: Flags::new(flags),
        })
    }

    fn flags(&self) -> Flags<ContinuationFlag> {
        self.flags
    }

    fn get_stream_id(&self) -> StreamId {
        self.stream_id
    }

    fn get_header(&self) -> FrameHeader {
        FrameHeader {
            length: self.payload_len(),
            frame_type: CONTINUATION_FRAME_TYPE,
            flags: self.flags.0,
            stream_id: self.stream_id,
        }
    }
}

impl FrameIR for ContinuationFrame {
    fn serialize_into<B : FrameBuilder>(self, b: &mut B) -> io::Result<()> {
        b.write_header(self.get_header())?;
        // Now the actual headers fragment
        b.write_all(&self.header_fragment)?;

        Ok(())
    }
}
