extern crate httpbis;

use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::Response;
use httpbis::Service;

// h2spec seems to expect response with body on `/`.
pub struct Ok200;

impl Service for Ok200 {
    fn start_request(&self, _headers: Headers, _req: HttpStreamAfterHeaders) -> Response {
        Response::found_200_plain_text("found")
    }
}
