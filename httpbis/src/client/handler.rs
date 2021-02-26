use bytes::Bytes;

use crate::client::resp::ClientResponse;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::error;
use crate::ClientRequest;
use crate::ErrorCode;
use crate::Headers;

/// Low-level client response handler.
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
    fn headers(&mut self, headers: Headers, end_stream: bool) -> crate::Result<()>;
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()>;
    /// Trailers HEADERS received
    fn trailers(self: Box<Self>, trailers: Headers) -> crate::Result<()>;
    /// RST_STREAM frame received
    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()>;
    /// Any other error
    fn error(self: Box<Self>, error: error::Error) -> crate::Result<()>;
}

pub(crate) struct ClientResponseStreamHandlerHolder(
    pub(crate) Box<dyn ClientResponseStreamHandler>,
);

impl StreamHandlerInternal for ClientResponseStreamHandlerHolder {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()> {
        self.0.data_frame(data, end_stream)
    }

    fn trailers(self, trailers: Headers) -> crate::Result<()> {
        self.0.trailers(trailers)
    }

    fn rst(self, error_code: ErrorCode) -> crate::Result<()> {
        self.0.rst(error_code)
    }

    fn error(self, error: error::Error) -> crate::Result<()> {
        self.0.error(error)
    }
}
