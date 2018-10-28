//! Tests for client.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

extern crate bytes;
extern crate env_logger;
extern crate futures;
extern crate httpbis;
extern crate log;
extern crate regex;
extern crate tokio_core;

extern crate httpbis_test;
use httpbis_test::*;

use bytes::Bytes;

use futures::future::Future;
use futures::stream::Stream;
use futures::sync::oneshot;

use tokio_core::reactor;

use futures::executor;
use futures::future;
use futures::Async;
use httpbis::for_test::solicit::DEFAULT_SETTINGS;
use httpbis::for_test::*;
use httpbis::ErrorCode;
use httpbis::*;

#[test]
fn stream_count() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    assert_eq!(0, client.conn_state().streams.len());

    let req = client
        .start_post("/foobar", "localhost", Bytes::from(&b"xxyy"[..]))
        .collect();

    let headers = server_tester.recv_frame_headers_check(1, false);
    assert_eq!("POST", headers.get(":method"));
    assert_eq!("/foobar", headers.get(":path"));

    let data = server_tester.recv_frame_data_check(1, true);
    assert_eq!(b"xxyy", &data[..]);

    let mut resp_headers = Headers::new();
    resp_headers.add(":status", "200");
    server_tester.send_headers(1, resp_headers, false);

    server_tester.send_data(1, b"aabb", true);

    let message = req.wait().expect("r");
    assert_eq!((b"aabb"[..]).to_owned(), message.body);

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn rst_is_error() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let req = client.start_get("/fgfg", "localhost").collect();

    let get = server_tester.recv_message(1);
    assert_eq!("GET", get.headers.method());

    server_tester.send_headers(1, Headers::ok_200(), false);
    server_tester.send_rst(1, ErrorCode::InadequateSecurity);

    match req.wait() {
        Ok(..) => panic!("expected error"),
        Err(Error::CodeError(ErrorCode::InadequateSecurity)) => {}
        Err(e) => panic!("wrong error: {:?}", e),
    }

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn handle_1xx_headers() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let req = client.start_get("/fgfg", "localhost").collect();

    let get = server_tester.recv_message(1);
    assert_eq!("GET", get.headers.method());

    server_tester.send_headers(1, Headers::from_status(100), false);
    server_tester.send_headers(1, Headers::from_status(100), false);

    server_tester.send_headers(1, Headers::ok_200(), false);

    server_tester.send_data(1, b"hello", true);

    req.wait().expect("Should be OK");

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn client_call_dropped() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    {
        let req = client.start_get("/fgfg", "localhost");

        server_tester.recv_message(1);

        drop(req);

        server_tester.send_headers(1, Headers::ok_200(), true);
    }

    {
        let req = client.start_get("/fgfg", "localhost").collect();
        server_tester.recv_message(3);
        server_tester.send_headers(3, Headers::ok_200(), true);
        let resp = req.wait().expect("OK");
        assert_eq!(200, resp.headers.status());
    }

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn reconnect_on_disconnect() {
    init_logger();

    let (server, client) = HttpServerTester::new_with_client();

    let mut server_tester = server.accept_xchg();

    {
        let req = client.start_get("/111", "localhost").collect();
        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = req.wait().expect("OK");
        assert_eq!(200, resp.headers.status());
    }

    // drop server connection
    drop(server_tester);

    // waiting for client connection to die
    while let Ok(_) = client.dump_state().wait() {
        thread::sleep(Duration::from_millis(1));
    }

    {
        let req = client.start_get("/222", "localhost").collect();

        let mut server_tester = server.accept();
        server_tester.recv_preface();
        server_tester.settings_xchg_but_ack();

        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = req.wait().expect("OK");
        assert_eq!(200, resp.headers.status());
    }
}

#[test]
fn reconnect_on_goaway() {
    init_logger();

    let (server, client) = HttpServerTester::new_with_client();

    {
        let mut server_tester = server.accept_xchg();

        let req = client.start_get("/111", "localhost").collect();
        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = req.wait().expect("OK");
        assert_eq!(200, resp.headers.status());

        server_tester.send_goaway(1);

        server_tester.recv_eof();
    }

    {
        let connect = client.wait_for_connect();

        let mut server_tester = server.accept_xchg();

        connect.wait().expect("connect");

        let req = client.start_get("/111", "localhost").collect();

        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = req.wait().expect("OK");
        assert_eq!(200, resp.headers.status());
    }
}

#[test]
pub fn issue_89() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let r1 = client.start_get("/r1", "localhost");

    server_tester.recv_frame_headers_check(1, true);

    server_tester.send_headers(1, Headers::ok_200(), false);
    let (_, resp1) = r1.0.wait().unwrap();
    let mut resp1 = resp1.filter_data().wait();

    assert_eq!(
        server_tester.out_window_size.0,
        client.conn_state().in_window_size
    );

    let w = DEFAULT_SETTINGS.initial_window_size;
    assert_eq!(w as i32, client.conn_state().in_window_size);

    server_tester.send_data(1, &[17, 19], false);
    assert_eq!(2, resp1.next().unwrap().unwrap().len());

    // client does not send WINDOW_UPDATE on such small changes
    assert_eq!((w - 2) as i32, client.conn_state().in_window_size);

    let _r3 = client.start_get("/r3", "localhost");

    // This is the cause of issue #89
    assert_eq!(w as i32, client.stream_state(3).in_window_size);

    // Cannot reliably check that stream actually resets
}

#[test]
fn external_event_loop() {
    init_logger();

    let server = ServerTest::new();

    let port = server.port;

    let (tx, rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let t = thread::spawn(move || {
        let mut core = reactor::Core::new().expect("Core::new");

        let mut clients = Vec::new();
        for _ in 0..2 {
            let mut client = ClientBuilder::new_plain();
            client.set_addr((BIND_HOST, port)).expect("set_addr");
            client.event_loop = Some(core.remote());
            clients.push(client.build().expect("client"));
        }

        tx.send(clients).expect("send clients");

        core.run(shutdown_rx.map_err(|_| panic!("aaa")))
            .expect("run");
    });

    for client in rx.recv().expect("rx") {
        let get = client.start_get("/echo", "localhost");
        assert_eq!(200, get.collect().wait().expect("get").headers.status());
    }

    shutdown_tx.send(()).expect("send");

    t.join().expect("join");
}

#[test]
pub fn sink_poll() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let (mut sender, _response) = client
        .start_post_sink("/foo", "sink")
        .wait()
        .expect("start_post_sink");

    server_tester.recv_frame_headers_check(1, false);

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(65535, client.conn_state().out_window_size);
    assert_eq!(65535, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(65535, client.stream_state(1).pump_out_window_size);

    assert_eq!(Ok(Async::Ready(())), sender.poll());

    let b = Bytes::from(vec![1; 65_535]);
    sender.send_data(b.clone()).expect("send_data");

    assert_eq!(
        b,
        Bytes::from(server_tester.recv_frames_data_check(1, 16_384, 65_535, false))
    );

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(0, client.conn_state().out_window_size);
    assert_eq!(0, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(0, client.stream_state(1).out_window_size);
    assert_eq!(0, client.stream_state(1).pump_out_window_size);

    let mut sender = executor::spawn(future::lazy(move || {
        assert_eq!(Ok(Async::NotReady), sender.poll());
        future::ok::<_, ()>(sender)
    })).wait_future()
    .unwrap();

    server_tester.send_window_update_conn(3);
    server_tester.send_window_update_stream(1, 5);

    sender.wait().unwrap();

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(3, client.conn_state().out_window_size);
    assert_eq!(3, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(5, client.stream_state(1).out_window_size);
    assert_eq!(5, client.stream_state(1).pump_out_window_size);

    let b = Bytes::from(vec![11, 22]);
    sender.send_data(b.clone()).expect("send_data");
    assert_eq!(
        b,
        Bytes::from(server_tester.recv_frame_data_check(1, false))
    );

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(1, client.conn_state().out_window_size);
    assert_eq!(1, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(3, client.stream_state(1).out_window_size);
    assert_eq!(3, client.stream_state(1).pump_out_window_size);

    sender.wait().unwrap();

    let b = Bytes::from(vec![33, 44]);
    sender.send_data(b.clone()).expect("send_data");
    assert_eq!(
        b.slice(0, 1),
        Bytes::from(server_tester.recv_frame_data_check(1, false))
    );

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(0, client.conn_state().out_window_size);
    assert_eq!(-1, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(2, client.stream_state(1).out_window_size);
    assert_eq!(1, client.stream_state(1).pump_out_window_size);
}

#[test]
fn sink_reset_by_peer() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let (mut sender, _response) = client
        .start_post_sink("/foo", "sink")
        .wait()
        .expect("start_post_sink");

    server_tester.recv_frame_headers_check(1, false);

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(65535, client.conn_state().out_window_size);
    assert_eq!(65535, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(65535, client.stream_state(1).out_window_size);
    assert_eq!(65535, client.stream_state(1).pump_out_window_size);

    assert_eq!(Ok(Async::Ready(())), sender.poll());

    let b = Bytes::from(vec![1; 65_535 * 2]);
    sender.send_data(b.clone()).expect("send_data");

    assert_eq!(
        b.slice(0, 65_535),
        Bytes::from(server_tester.recv_frames_data_check(1, 16_384, 65_535, false))
    );

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(0, client.conn_state().out_window_size);
    assert_eq!(-65535, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(0, client.stream_state(1).out_window_size);
    assert_eq!(-65535, client.stream_state(1).pump_out_window_size);

    server_tester.send_rst(1, ErrorCode::Cancel);

    while client.conn_state().streams.len() != 0 {
        // spin-wait
    }

    // pump out window must be reset to out window

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(0, client.conn_state().out_window_size);
    assert_eq!(0, client.conn_state().pump_out_window_size);

    // check that if more data is sent, pump_out_window_size is not exhausted

    let b = Bytes::from(vec![1; 100_000]);
    sender.send_data(b.clone()).expect("send_data");

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(0, client.conn_state().out_window_size);
    assert_eq!(0, client.conn_state().pump_out_window_size);
}
