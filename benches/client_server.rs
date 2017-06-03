#![feature(test)]

extern crate test;
extern crate httpbis;
extern crate futures;
extern crate bytes;

use httpbis::*;

use futures::stream;
use futures::stream::Stream;
use futures::future::Future;

use bytes::Bytes;

use test::Bencher;


struct Megabyte;

impl Service for Megabyte {
    fn start_request(&self, _headers: Headers, _req: HttpPartStream) -> Response {
        Response::headers_and_bytes_stream(
            Headers::ok_200(),
            stream::iter((0..1024).map(|i| Ok(Bytes::from(vec![(i % 0xff) as u8; 1024])))))
    }
}

#[bench]
fn bench(b: &mut Bencher) {
    b.iter(|| {
        let server = Server::new_plain(
            "[::1]:0",
            Default::default(),
            Megabyte);

        let client = Client::new_plain(
            "::1",
            server.local_addr().port(),
            Default::default())
                .unwrap();

        let (header, body) = client.start_get("/any", "localhost").0.wait().expect("headers");
        assert_eq!(200, header.status());

        let mut s = 0;
        for p in body.check_only_data().wait() {
            s += p.expect("body").len();
        }

        assert_eq!(1024 * 1024, s);
    })
}
