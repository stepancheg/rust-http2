use bytes::IntoBuf;
use bytes::BigEndian;
use bytes::Buf;

use solicit::StreamId;
use solicit::frame::Frame;
use solicit::frame::FrameHeader;
use solicit::frame::RawFrame;
use solicit::frame::flags::NoFlag;
use solicit::frame::flags::Flags;


#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PriorityFrame {
    flags: Flags<NoFlag>,
    pub stream_id: StreamId,
    pub exclusive: bool,
    pub stream_dep: StreamId,
    pub weight: u8,
}

pub const PRIORITY_FRAME_TYPE: u8 = 0x2;

impl Frame for PriorityFrame {
    type FlagType = NoFlag;

    fn from_raw(raw_frame: &RawFrame) -> Option<Self> {
        let FrameHeader { length, frame_type, flags, stream_id } = raw_frame.header();
        if length != 5 {
            return None;
        }
        if frame_type != PRIORITY_FRAME_TYPE {
            return None;
        }
        if flags != 0 {
            return None;
        }
        if stream_id == 0 {
            return None;
        }

        let mut payload = raw_frame.payload().into_buf();
        let first = payload.get_u32::<BigEndian>();
        let exclusive = (first & 0x80000000) != 0;
        let stream_dep = first & !0x80000000;
        let weight = payload.get_u8();
        assert_eq!(0, payload.remaining());

        Some(PriorityFrame {
            flags: Flags::new(flags),
            stream_id: stream_id,
            exclusive: exclusive,
            stream_dep: stream_dep,
            weight: weight,
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
            length: 5,
            frame_type: PRIORITY_FRAME_TYPE,
            flags: self.flags.0,
            stream_id: self.stream_id,
        }
    }
}
