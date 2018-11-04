use bytes::Bytes;
use common::stream_handler::StreamHandler;
use error;
use result;
use ErrorCode;
use Headers;

/// Synchrnous callback of incoming data
pub trait ServerStreamHandler: 'static {
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()>;
    /// Trailers HEADERS received
    fn trailers(&mut self, trailers: Headers) -> result::Result<()>;
    /// RST_STREAM frame received
    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()>;
    /// Any other error
    fn error(&mut self, error: error::Error) -> result::Result<()>;
}

impl<T: ServerStreamHandler> StreamHandler for T {
    fn headers(&mut self, _headers: Headers, _end_stream: bool) -> result::Result<()> {
        unreachable!("no headers for server")
    }

    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()> {
        self.data_frame(data, end_stream)
    }

    fn trailers(&mut self, trailers: Headers) -> result::Result<()> {
        self.trailers(trailers)
    }

    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()> {
        self.rst(error_code)
    }

    fn error(&mut self, error: error::Error) -> result::Result<()> {
        self.error(error)
    }
}
