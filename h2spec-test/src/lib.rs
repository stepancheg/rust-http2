extern crate httpbis;

use httpbis::Headers;
use httpbis::HttpStreamAfterHeaders;
use httpbis::ServerSender;
use httpbis::Service;
use httpbis::ServiceContext;
use httpbis::SimpleHttpMessage;

// h2spec seems to expect response with body on `/`.
pub struct Ok200;

impl Service for Ok200 {
    fn start_request(
        &self,
        _context: ServiceContext,
        _headers: Headers,
        _req: HttpStreamAfterHeaders,
        mut resp: ServerSender,
    ) -> httpbis::Result<()> {
        resp.send_message(SimpleHttpMessage::found_200_plain_text("found"))?;
        Ok(())
    }
}
