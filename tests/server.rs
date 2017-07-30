//! Tests for server.

extern crate regex;
extern crate bytes;
extern crate futures;
extern crate tokio_core;
extern crate tokio_tls_api;
extern crate httpbis;
#[macro_use]
extern crate log;
extern crate env_logger;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use bytes::Bytes;

use tokio_core::reactor;

mod test_misc;

use std::io::Write as _Write;
use std::io::Read as _Read;
use std::thread;

use futures::Async;
use futures::Poll;
use futures::future::Future;
use futures::stream::Stream;
use futures::stream;
use futures::sync::oneshot;

use httpbis::*;
use httpbis::stream_part::HttpStreamPart;
use httpbis::solicit::frame::settings::*;
use httpbis::solicit::frame::headers::*;
use httpbis::solicit::DEFAULT_SETTINGS;

use std::iter::FromIterator;
use std::net::TcpStream;
use std::sync::mpsc;

#[cfg(unix)]
extern crate unix_socket;
#[cfg(unix)]
use unix_socket::UnixStream;

use test_misc::*;


#[test]
fn simple_new() {
    env_logger::init().ok();

    let server = ServerOneConn::new_fn(0, |_headers, req| {
        Response::headers_and_stream(Headers::ok_200(), req)
    });

    let mut tester = HttpConnectionTester::connect(server.port());
    tester.send_preface();
    tester.settings_xchg();

    let mut headers = Headers::new();
    headers.add(":method", "GET");
    headers.add(":path", "/aabb");
    tester.send_headers(1, headers, false);

    tester.send_data(1, b"abcd", true);

    let recv_headers = tester.recv_frame_headers_check(1, false);
    assert_eq!("200", recv_headers.get(":status"));

    assert_eq!(&b"abcd"[..], &tester.recv_frame_data_check(1, true)[..]);

    assert_eq!(0, server.dump_state().streams.len());
}

#[test]
fn panic_in_handler() {
    env_logger::init().ok();

    let server = ServerOneConn::new_fn(0, |headers, _req| {
        if headers.path() == "/panic" {
            panic!("requested");
        } else {
            Response::headers_and_bytes(Headers::ok_200(), Bytes::from("hi there"))
        }
    });

    let mut tester = HttpConnectionTester::connect(server.port());
    tester.send_preface();
    tester.settings_xchg();

    {
        let resp = tester.get(1, "/hello");
        assert_eq!(200, resp.headers.status());
        assert_eq!(&b"hi there"[..], &resp.body[..]);
    }

    {
        let resp = tester.get(3, "/panic");
        assert_eq!(500, resp.headers.status());
    }

    {
        let resp = tester.get(5, "/world");
        assert_eq!(200, resp.headers.status());
        assert_eq!(&b"hi there"[..], &resp.body[..]);
    }

    assert_eq!(0, server.dump_state().streams.len());
}

#[test]
fn panic_in_stream() {
    env_logger::init().ok();

    let server = ServerOneConn::new_fn(0, |headers, _req| {
        if headers.path() == "/panic" {
            Response::from_stream(stream::iter((0..2).map(|i| {
                match i {
                    0 => Ok(HttpStreamPart::intermediate_headers(Headers::ok_200())),
                    _ => panic!("should reset stream"),
                }
            })))
        } else {
            Response::headers_and_bytes(Headers::ok_200(), Bytes::from("hi there"))
        }
    });

    let mut tester = HttpConnectionTester::connect(server.port());
    tester.send_preface();
    tester.settings_xchg();

    {
        let resp = tester.get(1, "/hello");
        assert_eq!(200, resp.headers.status());
        assert_eq!(&b"hi there"[..], &resp.body[..]);
    }

    {
        tester.send_get(3, "/panic");
        tester.recv_frame_headers_check(3, false);
        tester.recv_rst_frame();
    }

    {
        let resp = tester.get(5, "/world");
        assert_eq!(200, resp.headers.status());
        assert_eq!(&b"hi there"[..], &resp.body[..]);
    }

    assert_eq!(0, server.dump_state().streams.len());
}

#[test]
fn response_large() {
    env_logger::init().ok();

    let mut large_resp = Vec::new();
    while large_resp.len() < 100_000 {
        if large_resp.len() != 0 {
            write!(&mut large_resp, ",").unwrap();
        }
        let len = large_resp.len();
        write!(&mut large_resp, "{}", len).unwrap();
    }

    let large_resp_copy = large_resp.clone();

    let server = ServerOneConn::new_fn(0, move |_headers, _req| {
        Response::headers_and_bytes(Headers::ok_200(), Bytes::from(large_resp_copy.clone()))
    });

    // TODO: rewrite with TCP
    let client = Client::new_plain("::1", server.port(), Default::default()).expect("connect");
    let resp = client.start_post("/foobar", "localhost", Bytes::from(&b""[..])).collect().wait().expect("wait");
    assert_eq!(large_resp.len(), resp.body.len());
    assert_eq!((large_resp.len(), &large_resp[..]), (resp.body.len(), &resp.body[..]));

    assert_eq!(0, server.dump_state().streams.len());
}

#[test]
fn rst_stream_on_data_without_stream() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    // DATA frame without open stream
    tester.send_data(11, &[10, 20, 30], false);

    tester.recv_rst_frame_check(11, ErrorCode::StreamClosed);

    let r = tester.get(1, "/echo");
    assert_eq!(200, r.headers.status());
}

#[test]
fn exceed_max_frame_size() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    tester.send_data(1, &[0; 17_000], false);

    tester.recv_eof();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    assert_eq!(200, tester.get(1, "/echo").headers.status());
}

#[test]
fn increase_frame_size() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    let mut frame = SettingsFrame::new();
    frame.settings.push(HttpSetting::MaxFrameSize(20000));
    tester.send_recv_settings(frame);

    tester.send_get(1, "/blocks/30000/1");
    assert_eq!(200, tester.recv_frame_headers_check(1, false).status());
    assert_eq!(20000, tester.recv_frame_data_check(1, false).len());
    assert_eq!(10000, tester.recv_frame_data_tail(1).len());
}

#[test]
fn exceed_window_size() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    let mut frame = SettingsFrame::new();
    frame.settings.push(HttpSetting::MaxFrameSize(tester.conn.peer_settings.initial_window_size + 5));
    tester.send_recv_settings(frame);

    let data = Vec::from_iter((0..tester.conn.peer_settings.initial_window_size + 3).map(|_| 2));

    // Deliberately set wrong out_windows_size so `send_data` wouldn't fail.
    tester.conn.out_window_size.0 += 10000000;
    tester.send_data(1, &data, false);
    tester.recv_eof();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    assert_eq!(200, tester.get(1, "/echo").headers.status());
}

#[test]
fn stream_window_gt_conn_window() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    let w = DEFAULT_SETTINGS.initial_window_size;

    // May need to be changed if server defaults are changed
    assert_eq!(w as i32, tester.conn.in_window_size.size());

    tester.send_recv_settings(SettingsFrame::from_settings(vec![
        HttpSetting::InitialWindowSize(w * 2),
        HttpSetting::MaxFrameSize(w * 10)]));

    // Now new stream window is gt than conn window

    let w = tester.conn.peer_settings.initial_window_size;
    tester.send_get(1, &format!("/blocks/{}/{}", w, 2));

    assert_eq!(200, tester.recv_frame_headers_check(1, false).status());
    assert_eq!(w as usize, tester.recv_frame_data_check(1, false).len());

    let server_sn = server.server.dump_state().wait().expect("state");
    assert_eq!(0, server_sn.single_conn().1.out_window_size);
    assert_eq!(w as i32, server_sn.single_conn().1.single_stream().1.out_window_size);

    tester.send_window_update_conn(w);

    assert_eq!(w as usize, tester.recv_frame_data_tail(1).len());
}

#[test]
fn do_not_poll_when_not_enough_window() {
    env_logger::init().ok();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_copy = polls.clone();

    let server = ServerOneConn::new_fn(0, move |_, _| {
        struct StreamImpl {
            polls: Arc<AtomicUsize>,
        }

        impl Stream for StreamImpl {
            type Item = Bytes;
            type Error = Error;

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                let polls = self.polls.fetch_add(1, Ordering::SeqCst);
                Ok(Async::Ready(match polls {
                    0 | 1 | 2 => Some(Bytes::from(vec![
                        polls as u8; DEFAULT_SETTINGS.initial_window_size as usize])),
                    _ => None,
                }))
            }
        }

        Response::headers_and_bytes_stream(Headers::ok_200(), StreamImpl {
            polls: polls_copy.clone(),
        })
    });

    let mut tester = HttpConnectionTester::connect(server.port());
    tester.send_preface();
    tester.settings_xchg();

    tester.send_recv_settings(SettingsFrame::from_settings(vec![
        HttpSetting::MaxFrameSize(DEFAULT_SETTINGS.initial_window_size * 5)]));

    tester.send_get(1, "/fgfg");
    assert_eq!(200, tester.recv_frame_headers_check(1, false).status());
    assert_eq!(
        DEFAULT_SETTINGS.initial_window_size as usize,
        tester.recv_frame_data_check(1, false).len());

    assert_eq!(2, polls.load(Ordering::SeqCst));
}

#[test]
pub fn server_sends_continuation_frame() {
    env_logger::init().ok();

    let mut headers = Headers::ok_200();
    for i in 0..1000 {
        headers.add(&format!("abcdefghijklmnop{}", i), &format!("ABCDEFGHIJKLMNOP{}", i));
    }

    let headers_copy = headers.clone();

    let server = ServerOneConn::new_fn(0, move |_headers, _req| {
        Response::headers_and_bytes(headers_copy.clone(), "there")
    });

    let mut tester = HttpConnectionTester::connect(server.port());
    tester.send_preface();
    tester.settings_xchg();

    tester.send_get(1, "/long-header-list");
    let (headers_frame, headers_recv, cont_count) = tester.recv_frame_headers_decode();
    assert!(headers_frame.flags.is_set(HeadersFlag::EndHeaders));
    assert!(cont_count > 0);
    assert_eq!(headers, headers_recv);

    assert_eq!(&b"there"[..], &tester.recv_frame_data_tail(1)[..]);
}

#[test]
pub fn http_1_1() {
    env_logger::init().ok();

    let server = ServerTest::new();

    let mut tcp_stream = TcpStream::connect(("::1", server.port)).expect("connect");

    tcp_stream.write_all(b"GET / HTTP/1.1\n").expect("write");

    let mut read = Vec::new();
    tcp_stream.read_to_end(&mut read).expect("read");
    assert!(&read.starts_with(b"HTTP/1.1 500 Internal Server Error\r\n"), "{:?}", httpbis::misc::BsDebug(&read));
}

#[cfg(unix)]
#[test]
pub fn http_1_1_unix() {
    env_logger::init().ok();

    let _server = ServerTestUnixSocket::new("/tmp/rust_http2_test".to_owned());

    let mut unix_stream = UnixStream::connect("/tmp/rust_http2_test").expect("connect");

    unix_stream.write_all(b"GET / HTTP/1.1\n").expect("write");

    let mut read = Vec::new();
    unix_stream.read_to_end(&mut read).expect("read");
    assert!(&read.starts_with(b"HTTP/1.1 500 Internal Server Error\r\n"), "{:?}", httpbis::misc::BsDebug(&read));
}

#[test]
fn external_event_loop() {
    env_logger::init().ok();

    let (tx, rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let t = thread::spawn(move || {
        let mut core = reactor::Core::new().expect("Core::new");

        let mut servers = Vec::new();
        for _ in 0..2 {
            let mut server = ServerBuilder::new_plain();
            server.event_loop = Some(core.remote());
            server.set_port(0);
            server.service.set_service_fn(
                "/",
                |_, _| Response::headers_and_bytes(Headers::ok_200(), "aabb"));
            servers.push(server.build().expect("server"));
        }

        tx.send(servers.iter().map(|s| s.local_addr().port()).collect::<Vec<_>>()).expect("send");

        core.run(shutdown_rx).expect("run");
    });

    let ports = rx.recv().expect("recv");

    for port in ports {
        let client = Client::new_plain("::1", port, ClientConf::new())
            .expect("client");
        let resp = client.start_get("/", "localhost").collect().wait().expect("ok");
        assert_eq!(b"aabb", &resp.body[..]);
    }

    shutdown_tx.send(()).expect("send");

    t.join().expect("thread join");
}
