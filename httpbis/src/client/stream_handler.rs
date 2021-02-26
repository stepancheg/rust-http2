use crate::client::resp::ClientResponse;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::error;
use crate::result;
use crate::ClientRequest;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;

/// Called once when stream is created
pub trait ClientHandler: Send + 'static {
    /// Called when stream is created
    fn request_created(
        self: Box<Self>,
        req: ClientRequest,
        resp: ClientResponse,
    ) -> crate::Result<()>;

    fn error(self: Box<Self>, error: crate::Error);
}

/// Synchrnous callback of incoming data
pub trait ClientResponseStreamHandler: Send + 'static {
    /// Response HEADERS frame received
    fn headers(&mut self, headers: Headers, end_stream: bool) -> result::Result<()>;
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()>;
    /// Trailers HEADERS received
    fn trailers(&mut self, trailers: Headers) -> result::Result<()>;
    /// RST_STREAM frame received
    fn rst(self: Box<Self>, error_code: ErrorCode) -> result::Result<()>;
    /// Any other error
    fn error(self: Box<Self>, error: error::Error) -> result::Result<()>;
}

pub(crate) struct ClientResponseStreamHandlerHolder(
    pub(crate) Box<dyn ClientResponseStreamHandler>,
);

impl StreamHandlerInternal for ClientResponseStreamHandlerHolder {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()> {
        self.0.data_frame(data, end_stream)
    }

    fn trailers(&mut self, trailers: Headers) -> result::Result<()> {
        self.0.trailers(trailers)
    }

    fn rst(self, error_code: ErrorCode) -> result::Result<()> {
        self.0.rst(error_code)
    }

    fn error(self, error: error::Error) -> result::Result<()> {
        self.0.error(error)
    }
}
