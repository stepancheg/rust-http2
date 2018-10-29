use data_or_trailers::HttpStreamAfterHeaders;
use result;
use solicit::header::Headers;
use tokio_core::reactor::Remote;
use ServerSender;

pub struct ServiceContext {
    pub(crate) loop_handle: Remote,
}

impl ServiceContext {
    // TODO: provide access to executor if there's any
    pub fn loop_remote(&self) -> Remote {
        self.loop_handle.clone()
    }
}

/// Central HTTP/2 service interface.
///
/// This trait should be implemented by server.
pub trait Service: Send + Sync + 'static {
    /// Start HTTP/2 request.
    ///
    /// `headers` param specifies initial request headers.
    /// `req` param contains asynchronous stream of request content,
    /// stream of zero or more `DATA` frames followed by optional
    /// trailer `HEADERS` frame.
    // TODO: add context parameter
    fn start_request(
        &self,
        context: ServiceContext,
        headers: Headers,
        req: HttpStreamAfterHeaders,
        resp: ServerSender,
    ) -> result::Result<()>;
}
