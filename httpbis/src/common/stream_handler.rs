use crate::error;
use crate::solicit::end_stream::EndStream;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;

/// Synchronous callback of incoming data
pub(crate) trait StreamHandlerInternal: Send + 'static {
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()>;
    /// Trailers HEADERS received
    fn trailers(self, trailers: Headers) -> crate::Result<()>;
    /// RST_STREAM frame received
    fn rst(self, error_code: ErrorCode) -> crate::Result<()>;
    /// Any other error
    fn error(self, error: error::Error) -> crate::Result<()>;
}
