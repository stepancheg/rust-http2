use crate::codec::http_decode_read::HttpFrameDecodedOrGoaway;
use crate::common::types::Types;

pub(crate) enum LoopEvent<T: Types> {
    ToWriteMessage(T::ToWriteMessage),
    Frame(HttpFrameDecodedOrGoaway),
    ExitLoop,
}
