use bytes::Bytes;
use bytes::BytesMut;

use crate::solicit::frame::unpack_header_from_slice;
use crate::solicit::frame::HeadersFlag;
use crate::solicit::frame::HeadersFrame;
use crate::solicit::frame::HttpFrame;
use crate::solicit::frame::PushPromiseFlag;
use crate::solicit::frame::PushPromiseFrame;
use crate::solicit::frame::RawFrame;
use crate::solicit::frame::RawHttpFrameType;
use crate::solicit::frame::FRAME_HEADER_LEN;
use crate::solicit::stream_id::StreamId;
use crate::ErrorCode;
use futures::task::Context;
use std::pin::Pin;
use std::task::Poll;
use tokio::io::AsyncRead;

/// Buffered read for reading HTTP/2 frames.
pub struct HttpFramedRead<R: AsyncRead + Unpin> {
    read: R,
    buf: BytesMut,
}

impl<R: AsyncRead + Unpin> HttpFramedRead<R> {
    pub fn new(read: R) -> HttpFramedRead<R> {
        HttpFramedRead {
            read,
            buf: BytesMut::new(),
        }
    }

    fn fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        let mut_self = self.get_mut();
        mut_self.buf.reserve(8192);
        let n = match tokio_util::io::poll_read_buf(
            Pin::new(&mut mut_self.read),
            cx,
            &mut mut_self.buf,
        )? {
            Poll::Ready(n) => n,
            Poll::Pending => return Poll::Pending,
        };
        if n == 0 {
            return Poll::Ready(Err(crate::Error::EofFromStream));
        }
        Poll::Ready(Ok(()))
    }

    fn fill_buff_to_at_least(
        &mut self,
        cx: &mut Context<'_>,
        at_least: usize,
    ) -> Poll<crate::Result<()>> {
        while self.buf.len() < at_least {
            if let Poll::Pending = Pin::new(&mut *self).fill_buf(cx)? {
                return Poll::Pending;
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_raw_frame(
        &mut self,
        cx: &mut Context<'_>,
        max_frame_size: u32,
    ) -> Poll<crate::Result<RawFrame>> {
        if let Poll::Pending = self.fill_buff_to_at_least(cx, FRAME_HEADER_LEN)? {
            return Poll::Pending;
        }

        let header = {
            let header = &self.buf[..FRAME_HEADER_LEN];
            unpack_header_from_slice(header)
        };

        if header.payload_len > max_frame_size {
            warn!(
                "closing conn because peer sent frame with size: {}, max_frame_size: {}",
                header.payload_len, max_frame_size
            );
            return Poll::Ready(Err(crate::Error::CodeError(ErrorCode::FrameSizeError)));
        }

        let total_len = FRAME_HEADER_LEN + header.payload_len as usize;

        if let Poll::Pending = self.fill_buff_to_at_least(cx, total_len)? {
            return Poll::Pending;
        }

        Poll::Ready(Ok(RawFrame {
            raw_content: self.buf.split_to(total_len).freeze(),
        }))
    }

    fn poll_http_frame(
        &mut self,
        cx: &mut Context<'_>,
        max_frame_size: u32,
    ) -> Poll<crate::Result<HttpFrame>> {
        match self.poll_raw_frame(cx, max_frame_size)? {
            Poll::Ready(frame) => Poll::Ready(Ok(HttpFrame::from_raw(&frame)?)),
            Poll::Pending => Poll::Pending,
        }
    }
}

enum ContinuableFrame {
    Headers(HeadersFrame),
    PushPromise(PushPromiseFrame),
}

struct Continuable {
    header_fragment: BytesMut,
    /// Note frame contatains a header fragment, but it is not used
    frame: ContinuableFrame,
}

impl Continuable {
    fn headers(header: HeadersFrame) -> Continuable {
        Continuable {
            header_fragment: BytesMut::from(&header.header_fragment[..]),
            frame: ContinuableFrame::Headers(header),
        }
    }

    fn push_promise(push_promise: PushPromiseFrame) -> Continuable {
        Continuable {
            header_fragment: BytesMut::from(&push_promise.header_fragment[..]),
            frame: ContinuableFrame::PushPromise(push_promise),
        }
    }

    fn into_frame(mut self) -> HttpFrame {
        let header_fragment = match &mut self.frame {
            ContinuableFrame::Headers(headers) => &mut headers.header_fragment,
            ContinuableFrame::PushPromise(push_promise) => &mut push_promise.header_fragment,
        };
        *header_fragment = self.header_fragment.freeze();
        match self.frame {
            ContinuableFrame::Headers(headers) => HttpFrame::Headers(headers),
            ContinuableFrame::PushPromise(push_promise) => HttpFrame::PushPromise(push_promise),
        }
    }

    fn extend_header_fragment(&mut self, bytes: Bytes) {
        self.header_fragment.extend_from_slice(&bytes[..]);
    }

    fn set_end_headers(&mut self) {
        match self.frame {
            ContinuableFrame::Headers(ref mut headers) => {
                headers.flags.set(HeadersFlag::EndHeaders)
            }
            ContinuableFrame::PushPromise(ref mut push_promise) => {
                push_promise.flags.set(PushPromiseFlag::EndHeaders)
            }
        }
    }

    fn get_stream_id(&self) -> StreamId {
        match self.frame {
            ContinuableFrame::Headers(ref headers) => headers.stream_id,
            ContinuableFrame::PushPromise(ref push_promise) => push_promise.stream_id,
        }
    }
}

pub struct HttpFramedJoinContinuationRead<R: AsyncRead + Unpin> {
    framed_read: HttpFramedRead<R>,
    // TODO: check total size is not exceeded some limit
    header_opt: Option<Continuable>,
}

impl<R: AsyncRead + Unpin> HttpFramedJoinContinuationRead<R> {
    pub fn new(read: R) -> Self {
        HttpFramedJoinContinuationRead {
            framed_read: HttpFramedRead::new(read),
            header_opt: None,
        }
    }

    pub fn poll_http_frame(
        &mut self,
        cx: &mut Context<'_>,
        max_frame_size: u32,
    ) -> Poll<crate::Result<HttpFrame>> {
        loop {
            let frame = match self.framed_read.poll_http_frame(cx, max_frame_size)? {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(frame) => frame,
            };

            match frame {
                HttpFrame::Headers(h) => {
                    if let Some(_) = self.header_opt {
                        return Poll::Ready(Err(crate::Error::ExpectingContinuationGot(
                            RawHttpFrameType::HEADERS,
                        )));
                    } else {
                        if h.flags.is_set(HeadersFlag::EndHeaders) {
                            return Poll::Ready(Ok(HttpFrame::Headers(h)));
                        } else {
                            self.header_opt = Some(Continuable::headers(h));
                            continue;
                        }
                    }
                }
                HttpFrame::PushPromise(p) => {
                    if let Some(_) = self.header_opt {
                        return Poll::Ready(Err(crate::Error::ExpectingContinuationGot(
                            RawHttpFrameType::PUSH_PROMISE,
                        )));
                    } else {
                        if p.flags.is_set(PushPromiseFlag::EndHeaders) {
                            return Poll::Ready(Ok(HttpFrame::PushPromise(p)));
                        } else {
                            self.header_opt = Some(Continuable::push_promise(p));
                            continue;
                        }
                    }
                }
                HttpFrame::Continuation(c) => {
                    if let Some(mut h) = self.header_opt.take() {
                        if h.get_stream_id() != c.stream_id {
                            return Poll::Ready(Err(
                                crate::Error::ExpectingContinuationGotDifferentStreamId(
                                    h.get_stream_id(),
                                    c.stream_id,
                                ),
                            ));
                        } else {
                            let header_end = c.is_headers_end();
                            h.extend_header_fragment(c.header_fragment);
                            if header_end {
                                h.set_end_headers();
                                return Poll::Ready(Ok(h.into_frame()));
                            } else {
                                self.header_opt = Some(h);
                                continue;
                            }
                        }
                    } else {
                        return Poll::Ready(Err(crate::Error::ContinuationFrameWithoutHeaders));
                    }
                }
                f => {
                    if let Some(_) = self.header_opt {
                        return Poll::Ready(Err(crate::Error::ExpectingContinuationGot(
                            f.frame_type(),
                        )));
                    } else {
                        return Poll::Ready(Ok(f));
                    }
                }
            };
        }
    }
}
