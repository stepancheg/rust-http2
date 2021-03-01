use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use std::thread;
use std::time::Duration;

use regex::Regex;

use bytes::Bytes;

use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::stream;
use futures::stream::StreamExt;

use httpbis::*;
use tokio::runtime::Runtime;

fn new_server() -> Server {
    let mut server = ServerBuilder::new_plain();
    server.service.set_service_fn("/200", |_, resp| {
        resp.send_found_200_plain_text("hi")?;
        Ok(())
    });
    server.service.set_service_fn("/inf", |req, resp| {
        let re = Regex::new("/inf/(\\d+)").expect("regex");
        let captures = re.captures(req.headers.path()).expect("captures");
        let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");

        let mut sink = resp.send_headers(Headers::ok_200())?;
        sink.pull_bytes_from_stream(stream::repeat(Bytes::from(vec![17; size as usize])).map(Ok))?;
        Ok(())
    });
    server.service.set_service_fn("/bq", |req, resp| {
        let re = Regex::new("/bq/(\\d+)").expect("regex");
        let captures = re.captures(req.headers.path()).expect("captures");
        let size: u32 = captures.get(1).expect("1").as_str().parse().expect("parse");

        let (tx, rx) = mpsc::channel(1);

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();

            let mut tx = tx;

            let b = Bytes::from(vec![17; size as usize]);
            loop {
                rt.block_on(tx.send(b.clone())).expect("send");
            }
        });

        let mut sink = resp.send_headers(Headers::ok_200())?;
        sink.pull_bytes_from_stream(rx.map(Ok))?;
        Ok(())
    });
    server.set_port(0);
    server.build().expect("server")
}

fn spawn<F: FnOnce(Client, Arc<AtomicBool>) + Send + 'static>(port: u16, f: F) -> Arc<AtomicBool> {
    let still_alive = Arc::new(AtomicBool::new(false));
    let still_alive_copy = still_alive.clone();
    thread::spawn(move || {
        let client = Client::new_plain("127.0.0.1", port, ClientConf::new()).expect("client");

        f(client, still_alive_copy)
    });
    still_alive
}

fn get_200(client: Client, still_alive: Arc<AtomicBool>) {
    let rt = Runtime::new().unwrap();
    loop {
        still_alive.store(true, Ordering::SeqCst);
        let r = rt
            .block_on(client.start_get_collect("/200", "localhost"))
            .expect("get");
        assert_eq!(200, r.headers.status());
    }
}

fn inf_impl(client: Client, still_alive: Arc<AtomicBool>, path: &str, size: usize) {
    let rt = Runtime::new().unwrap();
    loop {
        let (headers, resp) = rt
            .block_on(client.start_get(path, "localhost"))
            .expect("get");

        assert_eq!(200, headers.status());

        let mut resp = resp.filter_data();

        let exp = vec![17; size as usize];

        let iterations = 1_000_000_000 / size;

        for _ in 0..iterations {
            still_alive.store(true, Ordering::SeqCst);

            let mut v = Vec::new();
            while v.len() < size as usize {
                still_alive.store(true, Ordering::SeqCst);
                v.extend(rt.block_on(resp.next()).unwrap().unwrap());
            }
            assert_eq!(&exp[..], &v[..size]);
            v.drain(..size);
        }
    }
}

fn inf_100(client: Client, still_alive: Arc<AtomicBool>) {
    inf_impl(client, still_alive, "/inf/100", 100);
}

fn inf_10m(client: Client, still_alive: Arc<AtomicBool>) {
    inf_impl(client, still_alive, "/inf/10000000", 10000000);
}

fn bq_100(client: Client, still_alive: Arc<AtomicBool>) {
    inf_impl(client, still_alive, "/bq/100", 100);
}

fn bq_10m(client: Client, still_alive: Arc<AtomicBool>) {
    inf_impl(client, still_alive, "/bq/10000000", 10000000);
}

fn main() {
    let server = new_server();

    let get_200 = spawn(server.local_addr().port().unwrap(), get_200);
    let inf_100 = spawn(server.local_addr().port().unwrap(), inf_100);
    let inf_10m = spawn(server.local_addr().port().unwrap(), inf_10m);
    let bq_100 = spawn(server.local_addr().port().unwrap(), bq_100);
    let bq_10m = spawn(server.local_addr().port().unwrap(), bq_10m);

    let all = vec![
        ("get_200", get_200),
        ("inf_100", inf_100),
        ("inf_10m", inf_10m),
        ("bq_100", bq_100),
        ("bq_10m", bq_10m),
    ];

    loop {
        thread::sleep(Duration::from_secs(1));

        let mut any_dead = false;
        for &(n, ref a) in &all {
            if !a.load(Ordering::SeqCst) {
                println!("{} is probably dead", n);
                any_dead = true;
            }
            a.store(false, Ordering::SeqCst);
        }

        if !any_dead {
            println!("all alive");
        }
    }
}
