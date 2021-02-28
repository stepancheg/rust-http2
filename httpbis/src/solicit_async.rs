use std::future::Future;
use std::io;
use std::io::Read;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;

use futures::stream::Stream;

use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use tls_api::AsyncSocket;

use crate::solicit::frame::unpack_header;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::RawFrame;
use crate::solicit::frame::RawFrameRef;
use crate::solicit::frame::SettingsFrame;
use crate::solicit::frame::FRAME_HEADER_LEN;

use crate::misc::BsDebug;

pub type HttpFutureSend<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;
pub type HttpFutureStreamSend<T> = Pin<Box<dyn Stream<Item = crate::Result<T>> + Send + 'static>>;

/// Inefficient, but OK because used only in tests
pub fn recv_raw_frame_sync(read: &mut dyn Read, max_frame_size: u32) -> crate::Result<RawFrame> {
    let mut header_buf = [0; FRAME_HEADER_LEN];
    read.read_exact(&mut header_buf)?;
    let header = unpack_header(&header_buf);
    if header.payload_len > max_frame_size {
        return Err(crate::Error::PayloadTooLarge(
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

//#[allow(dead_code)]
//pub fn send_raw_frame<W: AsyncWrite + Send + 'static>(write: W, frame: RawFrame) -> HttpFuture<W> {
//    let bytes = frame.serialize();
//    Box::new(
//        write
//            .write_all(&bytes)
//            .map(|(w, _)| w)
//            .map_err(|e| e.into()),
//    )
//}

pub async fn send_frame<W, F>(write: &mut W, frame: F) -> crate::Result<()>
where
    W: AsyncWrite + Send + Unpin + 'static,
    F: FrameIR,
{
    let buf = frame.serialize_into_vec();
    debug!(
        "send frame {}",
        RawFrameRef { raw_content: &buf }.frame_type()
    );
    AsyncWriteExt::write_all(write, &buf).await?;
    Ok(())
}

static PREFACE: &'static [u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

async fn send_settings<W: AsyncWrite + Unpin + Send + 'static>(
    conn: &mut W,
    settings: SettingsFrame,
) -> crate::Result<()> {
    send_frame(conn, settings).await
}

pub async fn client_handshake<I: AsyncSocket>(
    conn: &mut I,
    settings: SettingsFrame,
) -> crate::Result<()> {
    debug!("send PREFACE");
    conn.write_all(PREFACE).await?;

    send_settings(conn, settings).await?;

    Ok(())
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
async fn recv_preface_or_handle_http_1<I>(conn: &mut I) -> crate::Result<()>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    struct Intermediate<'a, I: AsyncRead> {
        collected: Vec<u8>,
        conn: &'a mut I,
    }

    impl<'a, I: AsyncRead> Future for Intermediate<'a, I>
    where
        I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        type Output = crate::Result<bool>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            loop {
                // Read byte-by-byte
                // Note it could be slow
                let mut buf = [0u8];
                let mut buf_ptr: &mut [u8] = &mut buf;
                let count =
                    match tokio_util::io::poll_read_buf(Pin::new(&mut self.conn), cx, &mut buf_ptr)
                    {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Ok(count)) => count,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                    };

                if count == 0 {
                    let io_error = io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF");
                    return Poll::Ready(Err(crate::Error::from(io_error)));
                }

                let c = buf[0];

                if self.collected.len() == 0 && c == 0x16 {
                    return Poll::Ready(Err(crate::Error::InvalidFrame(format!(
                        "wrong fitst byte, likely TLS"
                    ))));
                }

                self.collected.push(c);

                if self.collected == PREFACE {
                    return Poll::Ready(Ok(false));
                }

                // TODO: check only for first \n
                if c == b'\n' {
                    if looks_like_http_1(&self.collected) {
                        return Poll::Ready(Ok(true));
                    }
                }

                if self.collected.len() == PREFACE.len() {
                    return Poll::Ready(Err(crate::Error::InvalidFrame(format!(
                        "wrong preface, likely TLS: {:?}",
                        BsDebug(&self.collected)
                    ))));
                }
            }
        }
    }

    let need_500 = Intermediate {
        conn,
        collected: Vec::new(),
    }
    .await?;

    if need_500 {
        conn.write_all(HTTP_1_500_RESPONSE).await?;

        return Err(crate::Error::RequestIsMadeUsingHttp1);
    }

    Ok(())
}

pub async fn server_handshake<I>(conn: &mut I, settings: SettingsFrame) -> crate::Result<()>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut preface_buf = Vec::with_capacity(PREFACE.len());
    preface_buf.resize(PREFACE.len(), 0);

    recv_preface_or_handle_http_1(conn).await?;
    send_settings(conn, settings).await?;

    Ok(())
}
