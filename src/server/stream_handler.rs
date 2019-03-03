use bytes::Bytes;
use common::stream_handler::StreamHandlerInternal;
use error;
use result;
use ErrorCode;
use Headers;

/// Synchronous callback of incoming data
pub trait ServerStreamHandler: 'static {
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()>;
    /// Trailers HEADERS received
    fn trailers(&mut self, trailers: Headers) -> result::Result<()>;
    /// RST_STREAM frame received
    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()> {
        self.error(error::Error::CodeError(error_code))
    }
    /// Any other error
    fn error(&mut self, error: error::Error) -> result::Result<()>;
}

pub(crate) struct ServerStreamHandlerHolder(pub(crate) Box<ServerStreamHandler>);

impl StreamHandlerInternal for ServerStreamHandlerHolder {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()> {
        self.0.data_frame(data, end_stream)
    }

    fn trailers(&mut self, trailers: Headers) -> result::Result<()> {
        self.0.trailers(trailers)
    }

    fn rst(mut self, error_code: ErrorCode) -> result::Result<()> {
        self.0.rst(error_code)
    }

    fn error(mut self, error: error::Error) -> result::Result<()> {
        self.0.error(error)
    }
}
