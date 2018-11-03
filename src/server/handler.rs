use data_or_trailers::HttpStreamAfterHeaders;
use result;
use solicit::header::Headers;
use tokio_core::reactor::Remote;
use ServerResponse;

pub struct ServerHandlerContext {
    pub(crate) loop_handle: Remote,
}

impl ServerHandlerContext {
    // TODO: provide access to executor if there's any
    pub fn loop_remote(&self) -> Remote {
        self.loop_handle.clone()
    }
}

/// Central HTTP/2 service interface.
///
/// This trait can be implemented by handler provided by user.
pub trait ServerHandler: Send + Sync + 'static {
    /// Start HTTP/2 request.
    ///
    /// `headers` param specifies initial request headers.
    /// `req` param contains asynchronous stream of request content,
    /// stream of zero or more `DATA` frames followed by optional
    /// trailer `HEADERS` frame.
    fn start_request(
        &self,
        context: ServerHandlerContext,
        headers: Headers,
        req: HttpStreamAfterHeaders,
        resp: ServerResponse,
    ) -> result::Result<()>;
}
