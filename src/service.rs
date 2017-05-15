use solicit::header::Headers;
use stream_part::HttpPartStream;
use resp::Response;


pub trait Service : Send + 'static {
    fn start_request(&self, headers: Headers, req: HttpPartStream) -> Response;
}
