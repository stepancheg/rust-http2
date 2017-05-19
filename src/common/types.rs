use solicit::StreamId;

use super::*;

/// Client or server type names for connection and stream
pub trait Types {
    type HttpStream : HttpStream;
    type HttpStreamSpecific : HttpStreamDataSpecific;
    type ConnDataSpecific : ConnDataSpecific;
    type ConnData : ConnInner;
    // Message sent to write loop
    type ToWriteMessage : From<CommonToWriteMessage>;

    /// First stream id used by either client or server
    fn first_id() -> StreamId;
}
