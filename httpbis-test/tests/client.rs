//! Tests for client.

use log::info;

use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use httpbis_test::*;

use bytes::Bytes;

use futures::channel::oneshot;
use futures::stream::StreamExt;

use futures::future;
use futures::future::TryFutureExt;

use httpbis::for_test::solicit::DEFAULT_SETTINGS;
use httpbis::for_test::*;
use httpbis::ErrorCode;
use httpbis::*;
use std::task::Poll;
use tokio::runtime::Runtime;

#[test]
fn stream_count() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    assert_eq!(0, client.conn_state().streams.len());

    let resp = client.start_post_collect("/foobar", "localhost", Bytes::from(&b"xxyy"[..]));
    let headers = server_tester.recv_frame_headers_check(1, false);
    assert_eq!("POST", headers.get(":method"));
    assert_eq!("/foobar", headers.get(":path"));

    let data = server_tester.recv_frame_data_check(1, true);
    assert_eq!(b"xxyy", &data[..]);

    let mut resp_headers = Headers::new();
    resp_headers.add(":status", "200");
    server_tester.send_headers(1, resp_headers, false);

    server_tester.send_data(1, b"aabb", true);

    let rt = Runtime::new().unwrap();

    let message = rt.block_on(resp).expect("r");
    assert_eq!(&b"aabb"[..], message.body.as_ref());

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn rst_is_error() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let req = client.start_get_collect("/fgfg", "localhost");

    let get = server_tester.recv_message(1);
    assert_eq!("GET", get.headers.method());

    server_tester.send_headers(1, Headers::ok_200(), false);
    server_tester.send_rst(1, ErrorCode::InadequateSecurity);

    let rt = Runtime::new().unwrap();

    match rt.block_on(req) {
        Ok(..) => panic!("expected error"),
        Err(Error::RstStreamReceived(ErrorCode::InadequateSecurity)) => {}
        Err(e) => panic!("wrong error: {:?}", e),
    }

    let state: ConnStateSnapshot = client.conn_state();
    assert_eq!(0, state.streams.len(), "{:?}", state);
}

#[test]
fn handle_1xx_headers() {
    init_logger();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let req = client.start_get_collect("/fgfg", "localhost");

    let get = server_tester.recv_message(1);
    assert_eq!("GET", get.headers.method());

    server_tester.send_headers(1, Headers::new_status(100), false);
    server_tester.send_headers(1, Headers::new_status(100), false);

    server_tester.send_headers(1, Headers::ok_200(), false);

    server_tester.send_data(1, b"hello", true);

    let rt = Runtime::new().unwrap();

    rt.block_on(req).expect("Should be OK");

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

    let rt = Runtime::new().unwrap();

    {
        let req = client.start_get_collect("/fgfg", "localhost");
        server_tester.recv_message(3);
        server_tester.send_headers(3, Headers::ok_200(), true);
        let resp = rt.block_on(req).expect("OK");
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

    let rt = Runtime::new().unwrap();

    {
        let req = client.start_get_collect("/111", "localhost");
        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = rt.block_on(req).expect("OK");
        assert_eq!(200, resp.headers.status());
    }

    // drop server connection
    drop(server_tester);

    // waiting for client connection to die
    while let Ok(_) = rt.block_on(client.dump_state()) {
        thread::sleep(Duration::from_millis(1));
    }

    {
        let req = client.start_get_collect("/222", "localhost");

        let mut server_tester = server.accept();
        server_tester.recv_preface();
        server_tester.settings_xchg_but_ack();

        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = rt.block_on(req).expect("OK");
        assert_eq!(200, resp.headers.status());
    }
}

#[test]
fn reconnect_on_goaway() {
    init_logger();

    let (server, client) = HttpServerTester::new_with_client();

    let rt = Runtime::new().unwrap();

    {
        let mut server_tester = server.accept_xchg();

        let req = client.start_get_collect("/111", "localhost");
        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = rt.block_on(req).expect("OK");
        assert_eq!(200, resp.headers.status());

        server_tester.send_goaway(1);

        server_tester.recv_eof();
    }

    {
        let connect = client.wait_for_connect();

        let mut server_tester = server.accept_xchg();

        rt.block_on(connect).expect("connect");

        let req = client.start_get_collect("/111", "localhost");

        server_tester.recv_message(1);
        server_tester.send_headers(1, Headers::ok_200(), true);
        let resp = rt.block_on(req).expect("OK");
        assert_eq!(200, resp.headers.status());
    }
}

#[test]
pub fn issue_89() {
    init_logger();

    let rt = Runtime::new().unwrap();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let r1 = client.start_get("/r1", "localhost");

    server_tester.recv_frame_headers_check(1, true);

    server_tester.send_headers(1, Headers::ok_200(), false);
    let (_, resp1) = rt.block_on(r1).unwrap();
    let mut resp1 = resp1.filter_data();

    assert_eq!(
        server_tester.out_window_size.size(),
        client.conn_state().in_window_size
    );

    let w = DEFAULT_SETTINGS.initial_window_size;
    assert_eq!(w as i32, client.conn_state().in_window_size);

    server_tester.send_data(1, &[17, 19], false);
    assert_eq!(2, rt.block_on(resp1.next()).unwrap().unwrap().len());

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

    let rt = Runtime::new().unwrap();

    let server = ServerTest::new();

    let port = server.port;

    let (tx, rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let t = thread::spawn(move || {
        let core = Runtime::new().expect("Core::new");

        let mut clients = Vec::new();
        for _ in 0..2 {
            let mut client = ClientBuilder::new_plain();
            client.set_addr((BIND_HOST, port)).expect("set_addr");
            client.event_loop = Some(core.handle().clone());
            clients.push(client.build().expect("client"));
        }

        tx.send(clients).expect("send clients");

        info!("block_on...");
        core.block_on(shutdown_rx.map_err(|_| panic!("aaa")))
            .expect("run");
        info!("block_on done.");
    });

    for client in rx.recv().expect("rx") {
        info!("client.start_get...");
        let get = client.start_get_collect("/echo", "localhost");
        assert_eq!(200, rt.block_on(get).expect("get").headers.status());
        info!("client.start_get done.");
    }

    shutdown_tx.send(()).expect("send");

    t.join().expect("join");
}

#[test]
pub fn sink_poll() {
    init_logger();

    let rt = Runtime::new().unwrap();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let (mut sender, _response) = rt
        .block_on(client.start_post_sink("/foo", "sink"))
        .expect("start_post_sink");

    server_tester.recv_frame_headers_check(1, false);

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(65535, client.conn_state().out_window_size);
    assert_eq!(65535, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(65535, client.stream_state(1).pump_out_window_size);

    assert_eq!(
        Poll::Ready(Ok(())),
        sender
            .poll(&mut NopRuntime::new().context())
            .map_err(|_| ())
    );

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

    let rt = Runtime::new().unwrap();

    let sender = rt.block_on(future::lazy(move |cx| {
        assert_eq!(Poll::Pending, sender.poll(cx).map_err(|_| ()));
        future::ok::<_, ()>(sender)
    }));
    let mut sender = rt.block_on(sender).unwrap();

    server_tester.send_window_update_conn(3);
    server_tester.send_window_update_stream(1, 5);

    rt.block_on(future::poll_fn(|cx| sender.poll(cx))).unwrap();

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

    rt.block_on(future::poll_fn(|cx| sender.poll(cx))).unwrap();

    let b = Bytes::from(vec![33, 44]);
    sender.send_data(b.clone()).expect("send_data");
    assert_eq!(
        b.slice(0..1),
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

    let rt = Runtime::new().unwrap();

    let (mut server_tester, client) = HttpConnTester::new_server_with_client_xchg();

    let (mut sender, _response) = rt
        .block_on(client.start_post_sink("/foo", "sink"))
        .expect("start_post_sink");

    server_tester.recv_frame_headers_check(1, false);

    assert_eq!(65535, client.conn_state().in_window_size);
    assert_eq!(65535, client.conn_state().out_window_size);
    assert_eq!(65535, client.conn_state().pump_out_window_size);
    assert_eq!(65535, client.stream_state(1).in_window_size);
    assert_eq!(65535, client.stream_state(1).out_window_size);
    assert_eq!(65535, client.stream_state(1).pump_out_window_size);

    assert_eq!(
        Poll::Ready(Ok(())),
        sender
            .poll(&mut NopRuntime::new().context())
            .map_err(|_| ())
    );

    let b = Bytes::from(vec![1; 65_535 * 2]);
    sender.send_data(b.clone()).expect("send_data");

    assert_eq!(
        b.slice(0..65_535),
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

#[test]
fn connection_refused() {
    init_logger();

    let rt = Runtime::new().unwrap();

    let client = Client::new_plain(BIND_HOST, 1, ClientConf::default()).unwrap();

    let err = rt
        .block_on(async { client.start_get_collect("/test", "localhost").await })
        .err()
        .unwrap();
    match err {
        httpbis::Error::ConnDied(e) => match &*e {
            httpbis::Error::IoError(e) => {
                assert_eq!(
                    io::ErrorKind::ConnectionRefused,
                    e.kind(),
                    "wrong io error: {:?}",
                    e
                );
            }
            e => panic!("wrong conn died error: {:?}", e),
        },
        e => panic!("wrong conn error: {:?}", e),
    }
}
