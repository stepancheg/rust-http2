use bytes::Buf;
use bytes::Bytes;

use crate::solicit::frame::builder::FrameBuilder;
use crate::solicit::frame::parse_padded_payload;
use crate::solicit::frame::Frame;
use crate::solicit::frame::FrameHeader;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::ParseFrameError;
use crate::solicit::frame::ParseFrameResult;
use crate::solicit::frame::RawFrame;

use super::flags::Flag;
use super::flags::Flags;
use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::stream_id::StreamId;

pub const PUSH_PROMISE_FRAME_TYPE: u8 = 0x5;

/// `PUSH_PROMISE` frame.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PushPromiseFrame {
    /// The set of flags for the frame, packed into a single byte.
    pub flags: Flags<PushPromiseFlag>,
    /// The ID of the stream with which this frame is associated
    pub stream_id: StreamId,
    /// Promised Stream ID
    pub promised_stream_id: StreamId,
    /// The header fragment bytes stored within the frame.
    pub header_fragment: Bytes,
    /// The length of the padding, if any.
    pub padding_len: u8,
}

/// `PUSH_PROMISE` frame flag.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum PushPromiseFlag {
    /// Flag
    EndHeaders = 0x4,
    /// Flag
    Padded = 0x8,
}

impl Flag for PushPromiseFlag {
    fn bitmask(&self) -> u8 {
        *self as u8
    }

    fn flags() -> &'static [PushPromiseFlag] {
        static FLAGS: &'static [PushPromiseFlag] =
            &[PushPromiseFlag::EndHeaders, PushPromiseFlag::Padded];
        FLAGS
    }
}

impl PushPromiseFrame {
    /// Returns the length of the payload of the current frame, including any
    /// possible padding in the number of bytes.
    fn payload_len(&self) -> u32 {
        let padding = if self.flags.is_set(PushPromiseFlag::Padded) {
            1 + self.padding_len as u32
        } else {
            0
        };

        let stream_id_len = 4;

        self.header_fragment.len() as u32 + stream_id_len + padding
    }
}

impl Frame for PushPromiseFrame {
    type FlagType = PushPromiseFlag;

    fn from_raw(raw_frame: &RawFrame) -> ParseFrameResult<PushPromiseFrame> {
        // Unpack the header
        let FrameHeader {
            payload_len,
            frame_type,
            flags,
            stream_id,
        } = raw_frame.header();
        // Check that the frame type is correct for this frame implementation
        if frame_type != PUSH_PROMISE_FRAME_TYPE {
            return Err(ParseFrameError::InternalError);
        }

        // Check that the length given in the header matches the payload
        // length; if not, something went wrong and we do not consider this a
        // valid frame.
        if (payload_len as usize) != raw_frame.payload().len() {
            return Err(ParseFrameError::InternalError);
        }

        let flags = Flags::new(flags);

        // +---------------+
        // |Pad Length? (8)|
        // +-+-------------+-----------------------------------------------+
        // |R|                  Promised Stream ID (31)                    |
        // +-+-----------------------------+-------------------------------+
        // |                   Header Block Fragment (*)                 ...
        // +---------------------------------------------------------------+
        // |                           Padding (*)                       ...
        // +---------------------------------------------------------------+

        let padded = flags.is_set(PushPromiseFlag::Padded);

        let (payload, padding_len) = parse_padded_payload(raw_frame.payload(), padded)?;

        let mut buf = &payload[..];

        let promised_stream_id = buf.get_u32();

        let header_fragment =
            payload.slice((payload_len as usize) - buf.remaining()..payload.len());

        Ok(PushPromiseFrame {
            header_fragment,
            stream_id,
            padding_len,
            flags,
            promised_stream_id,
        })
    }

    fn flags(&self) -> Flags<PushPromiseFlag> {
        self.flags
    }

    fn get_stream_id(&self) -> StreamId {
        self.stream_id
    }

    fn get_header(&self) -> FrameHeader {
        FrameHeader {
            payload_len: self.payload_len(),
            frame_type: PUSH_PROMISE_FRAME_TYPE,
            flags: self.flags.0,
            stream_id: self.stream_id,
        }
    }
}

impl FrameIR for PushPromiseFrame {
    fn serialize_into(self, b: &mut WriteBuffer) {
        b.write_header(self.get_header());
        let padded = self.flags.is_set(PushPromiseFlag::Padded);
        if padded {
            b.extend_from_slice(&[self.padding_len]);
        }
        // Now the actual headers fragment
        b.extend_from_bytes(self.header_fragment);
        // Finally, add the trailing padding, if required
        if padded {
            b.write_padding(self.padding_len);
        }
    }
}
