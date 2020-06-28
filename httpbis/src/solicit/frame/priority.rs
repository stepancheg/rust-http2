use bytes::Buf;

use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::frame::flags::Flags;
use crate::solicit::frame::flags::NoFlag;
use crate::solicit::frame::Frame;
use crate::solicit::frame::FrameHeader;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::ParseFrameError;
use crate::solicit::frame::ParseFrameResult;
use crate::solicit::frame::RawFrame;
use crate::solicit::stream_id::StreamId;

/// `PRIORITY` frame.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PriorityFrame {
    /// Frame field
    flags: Flags<NoFlag>,
    /// Frame field
    pub stream_id: StreamId,
    /// Frame field
    pub exclusive: bool,
    /// Frame field
    pub stream_dep: StreamId,
    /// Frame field
    pub weight: u8,
}

pub const PRIORITY_FRAME_TYPE: u8 = 0x2;

impl Frame for PriorityFrame {
    type FlagType = NoFlag;

    fn from_raw(raw_frame: &RawFrame) -> ParseFrameResult<Self> {
        let FrameHeader {
            payload_len,
            frame_type,
            flags,
            stream_id,
        } = raw_frame.header();
        if payload_len != 5 {
            return Err(ParseFrameError::IncorrectFrameLength(payload_len));
        }
        if frame_type != PRIORITY_FRAME_TYPE {
            return Err(ParseFrameError::InternalError);
        }
        if flags != 0 {
            return Err(ParseFrameError::IncorrectFlags(flags));
        }
        if stream_id == 0 {
            return Err(ParseFrameError::StreamIdMustBeNonZero);
        }

        let mut payload = &raw_frame.payload()[..];
        let first = payload.get_u32();
        let exclusive = (first & 0x80000000) != 0;
        let stream_dep = first & !0x80000000;
        let weight = payload.get_u8();
        assert_eq!(0, payload.remaining());

        if stream_dep == stream_id {
            return Err(ParseFrameError::StreamDependencyOnItself(stream_id));
        }

        Ok(PriorityFrame {
            flags: Flags::new(flags),
            stream_id,
            exclusive,
            stream_dep,
            weight,
        })
    }

    fn flags(&self) -> Flags<NoFlag> {
        self.flags
    }

    fn get_stream_id(&self) -> StreamId {
        self.stream_id
    }

    fn get_header(&self) -> FrameHeader {
        FrameHeader {
            payload_len: 5,
            frame_type: PRIORITY_FRAME_TYPE,
            flags: self.flags.0,
            stream_id: self.stream_id,
        }
    }
}

impl FrameIR for PriorityFrame {
    fn serialize_into(self, _builder: &mut WriteBuffer) {
        unimplemented!()
    }
}
