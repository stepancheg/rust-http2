use codec::http_framed_read::HttpFramedJoinContinuationRead;
use error;
use futures::Async;
use futures::Poll;
use hpack;
use solicit::frame::headers::HeadersDecodedFrame;
use solicit::frame::HttpFrame;
use solicit::frame::HttpFrameDecoded;
use solicit::stream_id::StreamId;
use tokio_io::AsyncRead;
use ErrorCode;
use Header;
use Headers;

pub struct HttpDecodeRead<R: AsyncRead> {
    framed_read: HttpFramedJoinContinuationRead<R>,
    /// HPACK decoder used to decode incoming headers before passing them on to the session.
    decoder: hpack::Decoder,
}

pub enum HttpFrameDecodedOrGoaway {
    Frame(HttpFrameDecoded),
    SendGoaway(ErrorCode),
    _SendRst(StreamId, ErrorCode),
}

impl<R: AsyncRead> HttpDecodeRead<R> {
    pub fn new(read: R) -> Self {
        HttpDecodeRead {
            framed_read: HttpFramedJoinContinuationRead::new(read),
            decoder: hpack::Decoder::new(),
        }
    }

    pub fn poll_http_frame(
        &mut self,
        max_frame_size: u32,
    ) -> Poll<HttpFrameDecodedOrGoaway, error::Error> {
        let frame = match self.framed_read.poll_http_frame(max_frame_size)? {
            Async::Ready(frame) => frame,
            Async::NotReady => return Ok(Async::NotReady),
        };
        Ok(Async::Ready(HttpFrameDecodedOrGoaway::Frame(match frame {
            HttpFrame::Data(frame) => HttpFrameDecoded::Data(frame),
            HttpFrame::Headers(frame) => {
                let headers = match self.decoder.decode(&frame.header_fragment()) {
                    Err(e) => {
                        warn!("failed to decode headers: {:?}", e);
                        return Ok(Async::Ready(HttpFrameDecodedOrGoaway::SendGoaway(
                            ErrorCode::CompressionError,
                        )));
                    }
                    Ok(headers) => headers,
                };

                let headers = match headers
                    .into_iter()
                    .map(|h| Header::new_validate(h.0, h.1))
                    .collect::<Result<Vec<_>, _>>()
                    .and_then(Headers::from_vec_pseudo_first)
                {
                    Ok(headers) => headers,
                    Err(e) => {
                        // All pseudo-header fields MUST appear in the header block before
                        // regular header fields. Any request or response that contains
                        // a pseudo-header field that appears in a header block after
                        // a regular header field MUST be treated as malformed (Section 8.1.2.6).
                        warn!(
                            "received incorrect headers in stream {}: {:?}",
                            frame.stream_id, e
                        );
                        // TODO: close connection, because decoder may be in incorrect state
                        return Ok(Async::Ready(HttpFrameDecodedOrGoaway::SendGoaway(
                            ErrorCode::ProtocolError,
                        )));
                    }
                };

                HttpFrameDecoded::Headers(HeadersDecodedFrame {
                    flags: frame.flags,
                    stream_id: frame.stream_id,
                    headers,
                    stream_dep: frame.stream_dep,
                    padding_len: frame.padding_len,
                })
            }
            HttpFrame::Priority(frame) => HttpFrameDecoded::Priority(frame),
            HttpFrame::RstStream(frame) => HttpFrameDecoded::RstStream(frame),
            HttpFrame::Settings(frame) => HttpFrameDecoded::Settings(frame),
            HttpFrame::PushPromise(frame) => HttpFrameDecoded::PushPromise(frame),
            HttpFrame::Ping(frame) => HttpFrameDecoded::Ping(frame),
            HttpFrame::Goaway(frame) => HttpFrameDecoded::Goaway(frame),
            HttpFrame::WindowUpdate(frame) => HttpFrameDecoded::WindowUpdate(frame),
            HttpFrame::Continuation(_frame) => {
                unreachable!("must be joined with HEADERS before that")
            }
            HttpFrame::Unknown(frame) => HttpFrameDecoded::Unknown(frame),
        })))
    }
}
