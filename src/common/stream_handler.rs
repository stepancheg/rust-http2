use bytes::Bytes;
use error;
use result;
use ErrorCode;
use Headers;

/// Synchrnous callback of incoming data
pub(crate) trait StreamHandler: 'static {
    /// HEADERS frame received
    ///
    /// Invoked only on the client.
    fn headers(&mut self, headers: Headers, end_stream: bool) -> result::Result<()>;
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()>;
    /// Trailers HEADERS received
    fn trailers(&mut self, trailers: Headers) -> result::Result<()>;
    /// RST_STREAM frame received
    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()>;
    /// Any other error
    fn error(&mut self, error: error::Error) -> result::Result<()>;
}
