use solicit::StreamId;

use super::*;

/// Client or server type names for connection and stream
pub trait Types : 'static {
    type HttpStream : HttpStream;
    type HttpStreamSpecific : HttpStreamDataSpecific;
    type ConnDataSpecific : ConnDataSpecific;
    // Message sent to write loop
    type ToWriteMessage : From<CommonToWriteMessage>;

    /// First stream id used by either client or server
    fn first_id() -> StreamId;

    /// True if stream is initiated locally,
    /// e. g. `is_init_locally(3)` returns `true` for client and `false` for server.
    fn is_init_locally(stream_id: StreamId) -> bool {
        (stream_id % 2) == (Self::first_id() % 2)
    }
}
