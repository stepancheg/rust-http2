use crate::server::req::ServerRequest;
use crate::ServerResponse;

/// Server request callback.
///
/// This trait can be implemented by handler provided by user.
pub trait ServerHandler: Send + Sync + 'static {
    /// Start HTTP/2 request.
    ///
    /// `headers` param specifies initial request headers.
    /// `req` param contains asynchronous stream of request content,
    /// stream of zero or more `DATA` frames followed by optional
    /// trailer `HEADERS` frame.
    fn start_request(&self, req: ServerRequest, resp: ServerResponse) -> crate::Result<()>;
}
