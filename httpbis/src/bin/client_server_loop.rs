use bytes::Bytes;
use futures::stream::StreamExt;
use httpbis::Client;
use httpbis::ClientIntf;
use httpbis::Headers;
use httpbis::ServerBuilder;
use httpbis::ServerHandler;
use httpbis::ServerRequest;
use httpbis::ServerResponse;
use httpbis::SinkAfterHeaders;
use httpbis::StreamAfterHeaders;
use std::env;
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;

fn forever(mut cb: impl FnMut()) {
    println!("running forever");

    loop {
        let start = Instant::now();
        let mut limit = 1;
        let mut n = 0;
        loop {
            cb();

            n += 1;
            if n == limit {
                if start.elapsed().as_secs() >= 1 {
                    break;
                }
                limit *= 2;
            }
        }
        let per_iter = start.elapsed() / n;
        let us_per_iter = per_iter.as_secs() * 1_000_000 + per_iter.subsec_micros() as u64;
        println!("{}us per iter", us_per_iter);
    }
}

fn request() {
    struct My;

    impl ServerHandler for My {
        fn start_request(&self, _req: ServerRequest, resp: ServerResponse) -> httpbis::Result<()> {
            resp.send_found_200_plain_text("hello there")?;
            Ok(())
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service("/", Arc::new(My));
    let server = server.build().expect("server");

    let client = Client::new_plain(
        "127.0.0.1",
        server.local_addr().port().unwrap(),
        Default::default(),
    )
    .expect("client");

    let rt = Runtime::new().unwrap();
    forever(|| {
        rt.block_on(async {
            let (header, body) = client
                .start_get("/any", "localhost")
                .await
                .expect("headers");
            assert_eq!(200, header.status());

            let body = body.collect_data().await.expect("body");
            assert_eq!(&b"hello there"[..], &body[..]);
        })
    });
}

fn ping_pong() {
    const BLOCK_SIZE: usize = 1000;

    struct Echo;

    impl ServerHandler for Echo {
        fn start_request(&self, req: ServerRequest, resp: ServerResponse) -> httpbis::Result<()> {
            let mut sink = resp.send_headers(Headers::ok_200())?;
            sink.pull_from_stream(req.into_stream().into_stream())?;
            Ok(())
        }
    }

    let mut server = ServerBuilder::new_plain();
    server.set_port(0);
    server.service.set_service("/", Arc::new(Echo));
    let server = server.build().expect("server");

    let client = Client::new_plain(
        "127.0.0.1",
        server.local_addr().port().unwrap(),
        Default::default(),
    )
    .expect("client");

    let (mut sender, response) = Runtime::new()
        .unwrap()
        .block_on(client.start_post_sink("/any", "localhost"))
        .expect("request");

    let (header, response) = Runtime::new()
        .unwrap()
        .block_on(response)
        .expect("response wait");

    assert_eq!(200, header.status());

    let mut body = response.filter_data();
    let runtime = Runtime::new().unwrap();

    let mut i = 0u32;
    forever(move || {
        i = i.wrapping_add(1);
        let mut req = Vec::new();
        req.resize(BLOCK_SIZE, i as u8);
        sender.send_data(Bytes::from(req)).expect("send_data");

        let mut read = 0;
        while read < BLOCK_SIZE {
            let chunk = runtime.block_on(body.next()).unwrap().unwrap();
            read += chunk.len();
        }
        assert_eq!(BLOCK_SIZE, read);
    });
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match &args[1..] {
        &["request"] => request(),
        &["ping-pong"] => ping_pong(),
        _ => panic!("usage: {} <mode>", args[0]),
    }
}
