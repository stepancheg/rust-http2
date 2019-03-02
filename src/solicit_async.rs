use std::io;
use std::io::Read;

use bytes::Bytes;

use futures::Async;
use futures::Poll;

use futures::future;
use futures::future::Future;
use futures::stream::Stream;

use tokio_io::io::write_all;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;

use error;
use error::Error;
use result::Result;

use solicit::frame::settings::SettingsFrame;
use solicit::frame::unpack_header;
use solicit::frame::FrameIR;
use solicit::frame::RawFrame;
use solicit::frame::RawFrameRef;
use solicit::frame::FRAME_HEADER_LEN;

use misc::BsDebug;

pub type HttpFuture<T> = Box<Future<Item = T, Error = Error>>;

pub type HttpFutureSend<T> = Box<Future<Item = T, Error = Error> + Send>;
pub type HttpFutureStreamSend<T> = Box<Stream<Item = T, Error = Error> + Send>;

/// Inefficient, but OK because used only in tests
pub fn recv_raw_frame_sync(read: &mut Read, max_frame_size: u32) -> Result<RawFrame> {
    let mut header_buf = [0; FRAME_HEADER_LEN];
    read.read_exact(&mut header_buf)?;
    let header = unpack_header(&header_buf);
    if header.payload_len > max_frame_size {
        return Err(error::Error::PayloadTooLarge(
            header.payload_len,
            max_frame_size,
        ));
    }
    let total_length = FRAME_HEADER_LEN + header.payload_len as usize;
    let mut raw_frame = Vec::with_capacity(total_length);
    raw_frame.extend(&header_buf);
    raw_frame.resize(total_length, 0);
    read.read_exact(&mut raw_frame[FRAME_HEADER_LEN..])?;
    Ok(RawFrame {
        raw_content: Bytes::from(raw_frame),
    })
}

#[allow(dead_code)]
pub fn send_raw_frame<W: AsyncWrite + Send + 'static>(write: W, frame: RawFrame) -> HttpFuture<W> {
    let bytes = frame.serialize();
    Box::new(
        write_all(write, bytes.clone())
            .map(|(w, _)| w)
            .map_err(|e| e.into()),
    )
}

pub fn send_frame<W: AsyncWrite + Send + 'static, F: FrameIR>(
    write: W,
    frame: F,
) -> impl Future<Item = W, Error = error::Error> {
    let buf = frame.serialize_into_vec();
    debug!(
        "send frame {}",
        RawFrameRef { raw_content: &buf }.frame_type()
    );
    write_all(write, buf).map(|(w, _)| w).map_err(|e| e.into())
}

static PREFACE: &'static [u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

fn send_settings<W: AsyncWrite + Send + 'static>(
    conn: W,
    settings: SettingsFrame,
) -> HttpFuture<W> {
    Box::new(send_frame(conn, settings))
}

pub fn client_handshake<I: AsyncWrite + AsyncRead + Send + 'static>(
    conn: I,
    settings: SettingsFrame,
) -> HttpFuture<I> {
    debug!("send PREFACE");
    let send_preface = write_all(conn, PREFACE)
        .map(|(conn, _)| conn)
        .map_err(|e| e.into());

    let send_settings = send_preface.and_then(|conn| send_settings(conn, settings));

    Box::new(send_settings)
}

/// Response to be sent when request is sent over HTTP/1
const HTTP_1_500_RESPONSE: &'static [u8] = b"\
HTTP/1.1 500 Internal Server Error\r\n\
Server: httpbis\r\n\
\r\n\
Request is made using HTTP/1, server only supports HTTP/2\r\n\
";

/// Buf content looks like a start of HTTP/1 request
fn looks_like_http_1(buf: &[u8]) -> bool {
    buf.starts_with(b"GET ") || buf.starts_with(b"POST ") || buf.starts_with(b"HEAD ")
}

/// Recv HTTP/2 preface, or sent HTTP/1 500 and return error is input looks like HTTP/1 request
fn recv_preface_or_handle_http_1<I>(conn: I) -> HttpFuture<I>
where
    I: AsyncRead + AsyncWrite + Send + 'static,
{
    struct Intermediate<I: AsyncRead> {
        collected: Vec<u8>,
        conn: Option<I>,
    }

    impl<I: AsyncRead> Future for Intermediate<I>
    where
        I: AsyncRead + AsyncWrite + Send + 'static,
    {
        type Item = HttpFuture<I>;
        type Error = Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            loop {
                // Read byte-by-byte
                // Note it could be slow
                let mut buf = [0];
                let count = match self
                    .conn
                    .as_mut()
                    .expect("poll after completed")
                    .read(&mut buf)
                {
                    Ok(count) => count,
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(Async::NotReady);
                    }
                    Err(e) => return Err(e.into()),
                };

                if count == 0 {
                    let io_error = io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF");
                    return Err(error::Error::from(io_error));
                }

                let c = buf[0];

                if self.collected.len() == 0 && c == 0x16 {
                    return Err(Error::InvalidFrame(format!("wrong fitst byte, likely TLS")));
                }

                self.collected.push(c);

                if self.collected == PREFACE {
                    return Ok(Async::Ready(Box::new(future::ok(
                        self.conn.take().unwrap(),
                    ))));
                }

                // TODO: check only for first \n
                if c == b'\n' {
                    if looks_like_http_1(&self.collected) {
                        let w = write_all(self.conn.take().unwrap(), HTTP_1_500_RESPONSE);
                        let write = w.map_err(error::Error::from);
                        let write = write.then(|_| Err(error::Error::RequestIsMadeUsingHttp1));
                        return Ok(Async::Ready(Box::new(write)));
                    }
                }

                if self.collected.len() == PREFACE.len() {
                    return Err(error::Error::InvalidFrame(format!(
                        "wrong preface, likely TLS: {:?}",
                        BsDebug(&self.collected)
                    )));
                }
            }
        }
    }

    Box::new(
        Intermediate {
            conn: Some(conn),
            collected: Vec::new(),
        }
        .flatten(),
    )
}

pub fn server_handshake<I>(conn: I, settings: SettingsFrame) -> HttpFuture<I>
where
    I: AsyncRead + AsyncWrite + Send + 'static,
{
    let mut preface_buf = Vec::with_capacity(PREFACE.len());
    preface_buf.resize(PREFACE.len(), 0);

    let recv_preface = recv_preface_or_handle_http_1(conn);
    let send_settings = recv_preface.and_then(|conn| send_settings(conn, settings));

    Box::new(send_settings)
}
