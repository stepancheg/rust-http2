use Headers;
use HttpStreamAfterHeaders;

pub struct ServerRequest {
    pub headers: Headers,
    pub stream: HttpStreamAfterHeaders,
}
