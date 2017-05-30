//! Tests for server.

extern crate regex;
extern crate bytes;
extern crate futures;
extern crate native_tls;
extern crate tokio_core;
extern crate tokio_tls;
extern crate httpbis;
#[macro_use]
extern crate log;
extern crate env_logger;

use bytes::Bytes;

mod test_misc;

use std::io::Write as _Write;

use futures::Future;
use futures::stream;

use httpbis::solicit::header::*;

use httpbis::*;
use httpbis::stream_part::HttpStreamPart;
use httpbis::solicit::frame::settings::*;
use httpbis::solicit::frame::headers::*;
use httpbis::solicit::DEFAULT_SETTINGS;

use std::iter::FromIterator;

use test_misc::*;


#[test]
fn simple_new() {
    env_logger::init().ok();

    let server = HttpServerOneConn::new_fn(0, |_headers, req| {
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

    let server = HttpServerOneConn::new_fn(0, |headers, _req| {
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

    let server = HttpServerOneConn::new_fn(0, |headers, _req| {
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

    let server = HttpServerOneConn::new_fn(0, move |_headers, _req| {
        Response::headers_and_bytes(Headers::ok_200(), Bytes::from(large_resp_copy.clone()))
    });

    let client = Client::new("::1", server.port(), false, Default::default()).expect("connect");
    let resp = client.start_post("/foobar", "localhost", Bytes::from(&b""[..])).collect().wait().expect("wait");
    assert_eq!(large_resp.len(), resp.body.len());
    assert_eq!((large_resp.len(), &large_resp[..]), (resp.body.len(), &resp.body[..]));

    assert_eq!(0, server.dump_state().streams.len());
}

#[test]
fn rst_stream_on_data_without_stream() {
    env_logger::init().ok();

    let server = HttpServerTest::new();

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

    let server = HttpServerTest::new();

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

    let server = HttpServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    let mut frame = SettingsFrame::new();
    frame.settings.push(HttpSetting::MaxFrameSize(20000));
    tester.send_recv_settings(frame);

    tester.send_get(1, "/blocks/30000/1");
    assert_eq!(200, tester.recv_frame_headers_check(1, false).status());
    assert_eq!(20000, tester.recv_frame_data_check(1, false).len());
    assert_eq!(10000, tester.recv_frame_data_check(1, true).len());
}

#[test]
fn exceed_window_size() {
    env_logger::init().ok();

    let server = HttpServerTest::new();

    let mut tester = HttpConnectionTester::connect(server.port);
    tester.send_preface();
    tester.settings_xchg();

    let mut frame = SettingsFrame::new();
    frame.settings.push(HttpSetting::MaxFrameSize(tester.conn.peer_settings.initial_window_size + 5));
    tester.send_recv_settings(frame);

    let data = Vec::from_iter((0..tester.conn.peer_settings.initial_window_size + 3).map(|_| 2));

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

    let server = HttpServerTest::new();

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

    assert_eq!(w as usize, tester.recv_frame_data_check(1, true).len());
}

#[test]
pub fn server_sends_continuation_frame() {
    env_logger::init().ok();

    let mut headers = Headers::ok_200();
    for i in 0..1000 {
        headers.add(&format!("abcdefghijklmnop{}", i), &format!("ABCDEFGHIJKLMNOP{}", i));
    }

    let headers_copy = headers.clone();

    let server = HttpServerOneConn::new_fn(0, move |_headers, _req| {
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

    assert_eq!(&b"there"[..], &tester.recv_frame_data_check(1, true)[..]);
}
