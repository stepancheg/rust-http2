extern crate httpbis;

use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::ServerHandler;
use httpbis::ServerHandlerContext;
use httpbis::ServerResponse;
use httpbis::SimpleHttpMessage;

// h2spec seems to expect response with body on `/`.
pub struct Ok200;

impl ServerHandler for Ok200 {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        _headers: Headers,
        _req: HttpStreamAfterHeaders,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        resp.send_message(SimpleHttpMessage::found_200_plain_text("found"))?;
        Ok(())
    }
}
