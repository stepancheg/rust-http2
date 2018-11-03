use Headers;
use HttpStreamAfterHeaders;

pub struct ServerRequest {
    /// Request headers
    pub headers: Headers,
    /// True if requests ends with headers
    pub end_stream: bool,
    /// Request stream after headers
    ///
    /// Maybe empty e. g. for typical GET requests.
    pub stream: HttpStreamAfterHeaders,
}
