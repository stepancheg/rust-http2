use bytes::Bytes;
use bytes::BytesMut;

use error;
use futures::Async;
use futures::Poll;
use solicit::frame::headers::HeadersFlag;
use solicit::frame::push_promise::PushPromiseFlag;
use solicit::frame::unpack_header_from_slice;
use solicit::frame::HeadersFrame;
use solicit::frame::HttpFrame;
use solicit::frame::PushPromiseFrame;
use solicit::frame::RawFrame;
use solicit::StreamId;
use tokio_io::AsyncRead;
use ErrorCode;

/// Buffered read for reading HTTP/2 frames.
pub struct HttpFramedRead<R: AsyncRead> {
    read: R,
    buf: BytesMut,
}

pub const FRAME_HEADER_LEN: usize = 9;

impl<R: AsyncRead> HttpFramedRead<R> {
    pub fn new(read: R) -> HttpFramedRead<R> {
        HttpFramedRead {
            read,
            buf: BytesMut::new(),
        }
    }

    fn fill_buf(&mut self) -> Result<Async<()>, error::Error> {
        self.buf.reserve(8192);
        let n = match self.read.read_buf(&mut self.buf)? {
            Async::Ready(n) => n,
            Async::NotReady => return Ok(Async::NotReady),
        };
        if n == 0 {
            return Err(error::Error::Other("EOF from stream"));
        }
        Ok(Async::Ready(()))
    }

    fn fill_buff_to_at_least(&mut self, at_least: usize) -> Result<Async<()>, error::Error> {
        while self.buf.len() < at_least {
            if let Async::NotReady = self.fill_buf()? {
                return Ok(Async::NotReady);
            }
        }
        Ok(Async::Ready(()))
    }

    fn poll_raw_frame(&mut self, max_frame_size: u32) -> Result<Async<RawFrame>, error::Error> {
        if let Async::NotReady = self.fill_buff_to_at_least(FRAME_HEADER_LEN)? {
            return Ok(Async::NotReady);
        }

        let header = {
            let header = &self.buf[..FRAME_HEADER_LEN];
            unpack_header_from_slice(header)
        };

        if header.length > max_frame_size {
            warn!(
                "closing conn because peer sent frame with size: {}, max_frame_size: {}",
                header.length, max_frame_size
            );
            return Err(error::Error::CodeError(ErrorCode::FrameSizeError));
        }

        let total_len = FRAME_HEADER_LEN + header.length as usize;

        if let Async::NotReady = self.fill_buff_to_at_least(total_len)? {
            return Ok(Async::NotReady);
        }

        Ok(Async::Ready(RawFrame {
            raw_content: self.buf.split_to(total_len).freeze(),
        }))
    }

    fn poll_http_frame(&mut self, max_frame_size: u32) -> Result<Async<HttpFrame>, error::Error> {
        Ok(match self.poll_raw_frame(max_frame_size)? {
            Async::Ready(frame) => Async::Ready(HttpFrame::from_raw(&frame)?),
            Async::NotReady => Async::NotReady,
        })
    }
}

enum Continuable {
    Headers(HeadersFrame),
    PushPromise(PushPromiseFrame),
}

impl Continuable {
    fn into_frame(self) -> HttpFrame {
        match self {
            Continuable::Headers(headers) => HttpFrame::Headers(headers),
            Continuable::PushPromise(push_promise) => HttpFrame::PushPromise(push_promise),
        }
    }

    fn extend_header_fragment(&mut self, bytes: Bytes) {
        let header_fragment = match self {
            &mut Continuable::Headers(ref mut headers) => &mut headers.header_fragment,
            &mut Continuable::PushPromise(ref mut push_promise) => {
                &mut push_promise.header_fragment
            }
        };
        header_fragment.extend_from_slice(&bytes);
    }

    fn set_end_headers(&mut self) {
        match self {
            &mut Continuable::Headers(ref mut headers) => {
                headers.flags.set(HeadersFlag::EndHeaders)
            }
            &mut Continuable::PushPromise(ref mut push_promise) => {
                push_promise.flags.set(PushPromiseFlag::EndHeaders)
            }
        }
    }

    fn get_stream_id(&self) -> StreamId {
        match self {
            &Continuable::Headers(ref headers) => headers.stream_id,
            &Continuable::PushPromise(ref push_promise) => push_promise.stream_id,
        }
    }
}

pub struct HttpFramedJoinContinuationRead<R: AsyncRead> {
    framed_read: HttpFramedRead<R>,
    // TODO: check total size is not exceeded some limit
    header_opt: Option<Continuable>,
}

impl<R: AsyncRead> HttpFramedJoinContinuationRead<R> {
    pub fn new(read: R) -> Self {
        HttpFramedJoinContinuationRead {
            framed_read: HttpFramedRead::new(read),
            header_opt: None,
        }
    }

    pub fn poll_http_frame(&mut self, max_frame_size: u32) -> Poll<HttpFrame, error::Error> {
        loop {
            let frame = match self.framed_read.poll_http_frame(max_frame_size)? {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(frame) => frame,
            };

            match frame {
                HttpFrame::Headers(h) => {
                    if let Some(_) = self.header_opt {
                        return Err(error::Error::Other(
                            "expecting CONTINUATION frame, got HEADERS",
                        ));
                    } else {
                        if h.flags.is_set(HeadersFlag::EndHeaders) {
                            return Ok(Async::Ready(HttpFrame::Headers(h)));
                        } else {
                            self.header_opt = Some(Continuable::Headers(h));
                            continue;
                        }
                    }
                }
                HttpFrame::PushPromise(p) => {
                    if let Some(_) = self.header_opt {
                        return Err(error::Error::Other(
                            "expecting CONTINUATION frame, got PUSH_PROMISE",
                        ));
                    } else {
                        if p.flags.is_set(PushPromiseFlag::EndHeaders) {
                            return Ok(Async::Ready(HttpFrame::PushPromise(p)));
                        } else {
                            self.header_opt = Some(Continuable::PushPromise(p));
                            continue;
                        }
                    }
                }
                HttpFrame::Continuation(c) => {
                    if let Some(mut h) = self.header_opt.take() {
                        if h.get_stream_id() != c.stream_id {
                            return Err(error::Error::Other(
                                "CONTINUATION frame with different stream id",
                            ));
                        } else {
                            let header_end = c.is_headers_end();
                            h.extend_header_fragment(c.header_fragment);
                            if header_end {
                                h.set_end_headers();
                                return Ok(Async::Ready(h.into_frame()));
                            } else {
                                self.header_opt = Some(h);
                                continue;
                            }
                        }
                    } else {
                        return Err(error::Error::Other("CONTINUATION frame without headers"));
                    }
                }
                f => {
                    if let Some(_) = self.header_opt {
                        return Err(error::Error::Other("expecting CONTINUATION frame"));
                    } else {
                        return Ok(Async::Ready(f));
                    }
                }
            };
        }
    }
}
