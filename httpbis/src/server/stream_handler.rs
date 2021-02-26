use crate::common::stream_handler::StreamHandlerInternal;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;

/// Synchronous callback of incoming data
pub trait ServerRequestStreamHandler: Send + 'static {
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()>;
    /// Trailers HEADERS received
    fn trailers(self: Box<Self>, trailers: Headers) -> crate::Result<()>;
    /// RST_STREAM frame received
    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()> {
        self.error(crate::Error::CodeError(error_code))
    }
    /// Any other error
    fn error(self: Box<Self>, error: crate::Error) -> crate::Result<()>;
}

pub(crate) struct ServerRequestStreamHandlerHolder(pub(crate) Box<dyn ServerRequestStreamHandler>);

impl StreamHandlerInternal for ServerRequestStreamHandlerHolder {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()> {
        self.0.data_frame(data, end_stream)
    }

    fn trailers(self, trailers: Headers) -> crate::Result<()> {
        self.0.trailers(trailers)
    }

    fn rst(self, error_code: ErrorCode) -> crate::Result<()> {
        self.0.rst(error_code)
    }

    fn error(self, error: crate::Error) -> crate::Result<()> {
        self.0.error(error)
    }
}
