#![allow(dead_code)]

use std::io;
use std::io::Write;
use std::io::Read;

use std::str;
use std::net;
use std::net::ToSocketAddrs;

use bytes::Bytes;

use httpbis;
use httpbis::message::SimpleHttpMessage;
use httpbis::solicit::StreamId;
use httpbis::error::ErrorCode;
use httpbis::solicit::header::*;
use httpbis::solicit::frame::FrameIR;
use httpbis::solicit::frame::settings::SettingsFrame;
use httpbis::solicit::frame::headers::HeadersFrame;
use httpbis::solicit::frame::headers::HeadersFlag;
use httpbis::solicit::frame::continuation::ContinuationFrame;
use httpbis::solicit::frame::continuation::ContinuationFlag;
use httpbis::solicit::frame::data::DataFrame;
use httpbis::solicit::frame::data::DataFlag;
use httpbis::solicit::frame::goaway::GoawayFrame;
use httpbis::solicit::frame::window_update::WindowUpdateFrame;
use httpbis::solicit::frame::RawFrame;
use httpbis::solicit::frame::rst_stream::RstStreamFrame;
use httpbis::solicit::connection::HttpFrame;
use httpbis::solicit::connection::HttpConnection;

use super::BIND_HOST;


pub struct HttpServerTester(net::TcpListener);

impl HttpServerTester {
    pub fn on_port(port: u16) -> HttpServerTester {
        let socket = net::TcpListener::bind((BIND_HOST, port)).expect("bind");
        let server = HttpServerTester(socket);

        debug!("started HttpServerTester on {}", server.port());

        server
    }

    pub fn new() -> HttpServerTester {
        HttpServerTester::on_port(0)
    }

    pub fn port(&self) -> u16 {
        self.0.local_addr().unwrap().port()
    }

    pub fn accept(&self) -> HttpConnectionTester {
        debug!("accept connection...");
        let r = HttpConnectionTester {
            tcp: self.0.accept().unwrap().0,
            conn: HttpConnection::new(),
        };
        debug!("accept connection.");
        r
    }
}

static PREFACE: &'static [u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub struct HttpConnectionTester {
    tcp: net::TcpStream,
    pub conn: HttpConnection,
}

impl HttpConnectionTester {
    pub fn connect(port: u16) -> HttpConnectionTester {
        HttpConnectionTester {
            tcp: net::TcpStream::connect((BIND_HOST, port).to_socket_addrs().unwrap().next().unwrap())
                .expect("connect"),
            conn: HttpConnection::new(),
        }
    }

    pub fn recv_preface(&mut self) {
        let mut preface = Vec::new();
        preface.resize(PREFACE.len(), 0);
        self.tcp.read_exact(&mut preface).unwrap();
        assert_eq!(PREFACE, &preface[..]);
    }

    pub fn recv_eof(&mut self) {
        let r = self.tcp.read(&mut [0]);
        match r {
            Ok(0) => {}
            Ok(_) => panic!("expecting EOF"),
            Err(e) => {
                // On Linux it returns ECONNRESET
                if e.kind() != io::ErrorKind::ConnectionReset {
                    panic!("bad error");
                }
            }
        }
    }

    pub fn send_preface(&mut self) {
        self.tcp.write(PREFACE).expect("send");
    }

    pub fn send_frame<F : FrameIR>(&mut self, frame: F) {
        self.tcp.write(&frame.serialize_into_vec()).expect("send_frame");
    }

    pub fn send_window_update_conn(&mut self, increment: u32) {
        self.conn.in_window_size.try_increase(increment).unwrap();
        self.send_frame(WindowUpdateFrame::for_connection(increment));
    }

    pub fn send_goaway(&mut self, last_stream_id: StreamId) {
        self.send_frame(GoawayFrame::new(last_stream_id, ErrorCode::InadequateSecurity));
    }

    pub fn send_headers(&mut self, stream_id: StreamId, headers: Headers, end: bool) {
        let fragment = self.conn.encoder.encode(headers.0.iter().map(|h| (h.name(), h.value())));
        let mut headers_frame = HeadersFrame::new(fragment, stream_id);
        headers_frame.set_flag(HeadersFlag::EndHeaders);
        if end {
            headers_frame.set_flag(HeadersFlag::EndStream);
        }
        self.send_frame(headers_frame);
    }

    pub fn send_get(&mut self, stream_id: StreamId, path: &str) {
        let mut headers = Headers::new();
        headers.add(":method", "GET");
        headers.add(":path", path);
        headers.add(":scheme", "http");
        self.send_headers(stream_id, headers, true);
    }

    pub fn send_data(&mut self, stream_id: StreamId, data: &[u8], end: bool) {
        let mut data_frame = DataFrame::new(stream_id);
        data_frame.data = Bytes::from(data);
        if end {
            data_frame.set_flag(DataFlag::EndStream);
        }
        self.send_frame(data_frame);
        self.conn.out_window_size.try_decrease_to_positive(data.len() as i32).expect("decrease");
    }

    pub fn send_rst(&mut self, stream_id: StreamId, error_code: ErrorCode) {
        self.send_frame(RstStreamFrame::new(stream_id, error_code));
    }

    pub fn recv_raw_frame(&mut self) -> RawFrame {
        httpbis::solicit_async::recv_raw_frame_sync(
            &mut self.tcp,
            self.conn.our_settings_ack.max_frame_size)
                .expect("recv_raw_frame")
    }

    pub fn fn_recv_frame_no_check_ack(&mut self) -> HttpFrame {
        let raw_frame = self.recv_raw_frame();
        let frame = HttpFrame::from_raw(&raw_frame).expect("parse frame");
        debug!("received frame: {:?}", frame);
        frame
    }

    pub fn recv_special_frame_process_special(&mut self) -> Option<HttpFrame> {
        let frame = self.fn_recv_frame_no_check_ack();
        if let HttpFrame::Settings(ref f) = frame {
            if self.conn.our_settings_sent.is_some() && f.is_ack() {
                self.process_peer_settings_ack(&f);
                return None;
            }
        }
        if let HttpFrame::WindowUpdate(ref f) = frame {
            if f.stream_id == 0 {
                self.conn.out_window_size.try_increase(f.increment).expect("increment");
            } else {
                // TODO: store for use by test
            }
            return None;
        }
        Some(frame)
    }

    pub fn recv_frame(&mut self) -> HttpFrame {
        loop {
            if let Some(frame) = self.recv_special_frame_process_special() {
                return frame;
            }
        }
    }

    pub fn recv_frame_settings(&mut self) -> SettingsFrame {
        match self.fn_recv_frame_no_check_ack() {
            HttpFrame::Settings(settings) => settings,
            f => panic!("expecting SETTINGS, got: {:?}", f),
        }
    }

    pub fn recv_frame_settings_set(&mut self) -> SettingsFrame {
        let settings = self.recv_frame_settings();
        assert!(!settings.is_ack());
        self.conn.peer_settings.apply_from_frame(&settings);
        settings
    }

    fn process_peer_settings_ack(&mut self, frame: &SettingsFrame) {
        assert!(frame.is_ack());
        assert!(self.conn.our_settings_sent.is_some());
        self.conn.our_settings_ack = self.conn.our_settings_sent.take().unwrap();
    }

    pub fn recv_frame_settings_ack(&mut self) -> SettingsFrame {
        assert!(self.conn.our_settings_sent.is_some());
        let settings = self.recv_frame_settings();
        self.process_peer_settings_ack(&settings);
        settings
    }

    pub fn get(&mut self, stream_id: StreamId, path: &str) -> SimpleHttpMessage {
        self.send_get(stream_id, path);

        self.recv_message(stream_id)
    }

    pub fn send_settings(&mut self, settings: SettingsFrame) {
        assert!(self.conn.our_settings_sent.is_none());
        let mut new_settings = self.conn.our_settings_ack;
        new_settings.apply_from_frame(&settings);
        self.conn.our_settings_sent = Some(new_settings);
        self.send_frame(settings);
    }

    // Perform handshape, but do not wait for ACK of my SETTINGS
    // Useful, because ACK may come e.g. after first request HEADERS
    pub fn settings_xchg_but_ack(&mut self) {
        self.send_settings(SettingsFrame::new());
        self.recv_frame_settings_set();
        self.send_frame(SettingsFrame::new_ack());
    }

    pub fn settings_xchg(&mut self) {
        self.settings_xchg_but_ack();
        self.recv_frame_settings_ack();
    }

    pub fn send_recv_settings(&mut self, settings: SettingsFrame) {
        self.send_settings(settings);
        self.recv_frame_settings_ack();
    }

    pub fn recv_rst_frame(&mut self) -> RstStreamFrame {
        match self.recv_frame() {
            HttpFrame::RstStream(rst) => rst,
            f => panic!("expecting RST, got: {:?}", f),
        }
    }

    pub fn recv_goaway_frame(&mut self) -> GoawayFrame {
        match self.recv_frame() {
            HttpFrame::Goaway(goaway) => goaway,
            f => panic!("expecting GOAWAY, got: {:?}", f),
        }
    }

    pub fn recv_rst_frame_check(&mut self, stream_id: StreamId, error_code: ErrorCode) {
        let frame = self.recv_rst_frame();
        assert_eq!(stream_id, frame.stream_id);
        assert_eq!(error_code, frame.error_code());
    }

    pub fn recv_goaway_frame_check(&mut self, error_code: ErrorCode) {
        let frame = self.recv_goaway_frame();
        assert_eq!(error_code, frame.error_code());
    }

    fn recv_frame_continuation(&mut self) -> ContinuationFrame {
        match self.recv_frame() {
            HttpFrame::Continuation(continuation) => continuation,
            f => panic!("expecting CONTINUATION, got: {:?}", f),
        }
    }

    pub fn recv_frame_headers_continuation(&mut self) -> (HeadersFrame, u32) {
        let mut headers =
            match self.recv_frame() {
                HttpFrame::Headers(headers) => headers,
                f => panic!("expecting HEADERS, got: {:?}", f),
            };

        if headers.flags.is_set(HeadersFlag::EndHeaders) {
            return (headers, 0);
        }

        let mut cont_count = 0;

        loop {
            let continuation = self.recv_frame_continuation();
            cont_count += 1;

            headers.header_fragment.extend_from_slice(&continuation.header_fragment);

            if continuation.flags.is_set(ContinuationFlag::EndHeaders) {
                headers.set_flag(HeadersFlag::EndHeaders);
                return (headers, cont_count);
            }
        }
    }

    pub fn recv_frame_headers_decode(&mut self) -> (HeadersFrame, Headers, u32) {
        let (frame, cont_count) = self.recv_frame_headers_continuation();
        let headers = self.conn.decoder.decode(frame.header_fragment()).expect("decode");
        let headers = Headers(headers.into_iter().map(|(n, v)| Header::new(n, v)).collect());
        (frame, headers, cont_count)
    }

    pub fn recv_frame_headers_check(&mut self, stream_id: StreamId, end: bool) -> Headers {
        let (frame, headers, _) = self.recv_frame_headers_decode();
        assert_eq!(stream_id, frame.stream_id);
        assert_eq!(end, frame.is_end_of_stream());
        headers
    }

    pub fn recv_frame_data(&mut self) -> DataFrame {
        match self.recv_frame() {
            HttpFrame::Data(data) => data,
            f => panic!("expecting DATA, got: {:?}", f),
        }
    }

    pub fn recv_frame_data_check(&mut self, stream_id: StreamId, end: bool) -> Vec<u8> {
        let data = self.recv_frame_data();
        assert_eq!(stream_id, data.stream_id);
        assert_eq!(end, data.is_end_of_stream());
        (&data.data[..]).to_vec()
    }

    pub fn recv_frame_data_check_empty_end(&mut self, stream_id: StreamId) {
        let data = self.recv_frame_data_check(stream_id, true);
        assert!(data.is_empty());
    }

    /// Receive at most two frames till END_STREAM frame
    /// * if first frame has not END_STREAM, second must have empty payload
    pub fn recv_frame_data_tail(&mut self, stream_id: StreamId) -> Vec<u8> {
        let frame = self.recv_frame_data();
        assert_eq!(stream_id, frame.stream_id);

        let data = (&frame.data).to_vec();

        if frame.is_end_of_stream() {
            return data;
        }

        self.recv_frame_data_check_empty_end(stream_id);

        data
    }

    pub fn recv_message(&mut self, stream_id: StreamId) -> SimpleHttpMessage {
        let mut r = SimpleHttpMessage::default();
        loop {
            let frame = self.recv_frame();
            assert_eq!(stream_id, frame.get_stream_id());
            let end_of_stream = match frame {
                HttpFrame::Headers(headers_frame) => {
                    let end_of_stream = headers_frame.is_end_of_stream();
                    let headers = self.conn.decoder.decode(headers_frame.header_fragment()).expect("decode");
                    let headers = Headers(headers.into_iter().map(|(n, v)| Header::new(n, v)).collect());
                    r.headers.extend(headers);
                    end_of_stream
                }
                HttpFrame::Data(data_frame) => {
                    let end_of_stream = data_frame.is_end_of_stream();
                    r.body.extend_from_slice(&data_frame.data);
                    end_of_stream
                }
                frame => panic!("expecting HEADERS or DATA, got: {:?}", frame),
            };
            if end_of_stream {
                return r;
            }
        }
    }
}
