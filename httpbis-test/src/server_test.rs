#![allow(dead_code)]

use std::sync::Arc;

use bytes::Bytes;

use httpbis;
use httpbis::Headers;
use httpbis::Server;
use httpbis::ServerBuilder;
use httpbis::ServerHandler;

use futures::stream;
use httpbis::ServerHandlerContext;
use httpbis::ServerRequest;
use httpbis::ServerResponse;
use regex::Regex;

/// HTTP/2 server used by tests
pub struct ServerTest {
    pub server: Server,
    pub port: u16,
}

struct Blocks {}

impl ServerHandler for Blocks {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        req: ServerRequest,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        let blocks_re = Regex::new("^/blocks/(\\d+)/(\\d+)$").expect("regex");

        if let Some(captures) = blocks_re.captures(req.headers.path()) {
            let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");
            let count: u32 = captures.get(2).expect("2").as_str().parse().expect("parse");
            resp.send_headers(Headers::ok_200())?;
            let stream = stream::iter_ok(
                (0..count).map(move |i| Bytes::from(vec![(i % 0xff) as u8; size as usize])),
            );
            resp.pull_bytes_from_stream(stream)?;
        } else {
            resp.send_not_found_404("Only /blocks/ here")?;
        }

        Ok(())
    }
}

struct Echo {}

impl ServerHandler for Echo {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        mut req: ServerRequest,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        resp.send_headers(Headers::ok_200())?;
        resp.pull_from_stream(req.make_stream())?;
        Ok(())
    }
}

impl ServerTest {
    pub fn new() -> ServerTest {
        let mut server = ServerBuilder::new_plain();
        server.set_port(0);
        server.service.set_service("/blocks", Arc::new(Blocks {}));
        server.service.set_service("/echo", Arc::new(Echo {}));
        let server = server.build().expect("server");
        let port = server.local_addr().port().unwrap();
        ServerTest {
            server: server,
            port: port,
        }
    }

    #[cfg(unix)]
    pub fn new_unix(addr: String) -> ServerTest {
        let mut server = ServerBuilder::new_plain_unix();
        server.set_unix_addr(addr).unwrap();

        server.service.set_service("/blocks", Arc::new(Blocks {}));
        server.service.set_service("/echo", Arc::new(Echo {}));
        let server = server.build().expect("server");
        ServerTest {
            server: server,
            port: 0,
        }
    }
}
