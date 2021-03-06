use bytes::Bytes;

use crate::client::resp::ClientResponse;
use crate::common::sink_after_headers::SinkAfterHeadersBox;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::error;
use crate::solicit::end_stream::EndStream;
use crate::ErrorCode;
use crate::Headers;

/// Low-level client response handler.
///
/// This is a parameter to
/// [`start_request_low_level`](crate::ClientIntf::start_request_low_level) operation,
/// rarely needed to be used directly.
pub trait ClientHandler: Send + 'static {
    /// Called when stream is created
    fn request_created(
        self: Box<Self>,
        req: SinkAfterHeadersBox,
        resp: ClientResponse,
    ) -> crate::Result<()>;

    /// Could not start request (e. g. because of connection failure).
    fn error(self: Box<Self>, error: crate::Error);
}

/// Synchronous callback of incoming data
pub trait ClientResponseStreamHandler: Send + 'static {
    /// Response HEADERS frame received
    fn headers(&mut self, headers: Headers, end_stream: EndStream) -> crate::Result<()>;
    /// DATA frame received
    fn data_frame(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()>;
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
    fn data_frame(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()> {
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
