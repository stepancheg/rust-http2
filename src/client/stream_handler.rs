use bytes::Bytes;
use common::stream_handler::StreamHandlerInternal;
use error;
use result;
use ErrorCode;
use Headers;

/// Synchrnous callback of incoming data
pub trait ClientStreamHandler: 'static {
    fn request_created(&mut self) -> result::Result<()>;
    /// Response HEADERS frame received
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

pub(crate) struct ClientStreamHandlerHolder(pub(crate) Box<ClientStreamHandler>);

impl StreamHandlerInternal for ClientStreamHandlerHolder {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()> {
        self.0.data_frame(data, end_stream)
    }

    fn trailers(&mut self, trailers: Headers) -> result::Result<()> {
        self.0.trailers(trailers)
    }

    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()> {
        self.0.rst(error_code)
    }

    fn error(&mut self, error: error::Error) -> result::Result<()> {
        self.0.error(error)
    }
}
