use solicit::StreamId;

use super::*;
use req_resp::RequestOrResponse;
use tokio_io::AsyncWrite;
use tokio_io::AsyncRead;

/// Client or server type names for connection and stream
pub trait Types : 'static {
    type Io : AsyncWrite + AsyncRead + Send + 'static;
    type HttpStreamData: HttpStreamData;
    type HttpStreamSpecific : HttpStreamDataSpecific;
    type ConnDataSpecific : ConnSpecific;
    // Message sent to write loop
    type ToWriteMessage : From<CommonToWriteMessage> + Send;

    fn out_request_or_response() -> RequestOrResponse;

    fn in_request_or_response() -> RequestOrResponse {
        Self::out_request_or_response().invert()
    }

    /// First stream id used by either client or server
    fn first_id() -> StreamId;

    /// True if stream is initiated locally,
    /// e. g. `is_init_locally(3)` returns `true` for client and `false` for server.
    fn is_init_locally(stream_id: StreamId) -> bool {
        (stream_id % 2) == (Self::first_id() % 2)
    }
}
