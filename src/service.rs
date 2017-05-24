use solicit::header::Headers;
use stream_part::HttpPartStream;
use resp::Response;


/// HTTP/2 service interface
///
/// Implemented by `Client` and it is callback provided by user.
pub trait Service : Send + 'static {
    fn start_request(&self, headers: Headers, req: HttpPartStream) -> Response;
}
