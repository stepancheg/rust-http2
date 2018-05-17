use bytes::BytesMut;

use tokio_io::AsyncRead;
use error;
use futures::Async;
use futures::future::Future;
use solicit::frame::unpack_header_from_slice;
use ErrorCode;
use solicit::frame::RawFrame;
use solicit::connection::HttpFrame;


/// Buffered read for reading HTTP/2 frames.
pub struct HttpFrameRead<R : AsyncRead> {
    read: R,
    buf: BytesMut,
}


pub const FRAME_HEADER_LEN: usize = 9;


impl<R : AsyncRead> HttpFrameRead<R> {

    pub fn new(read: R) -> HttpFrameRead<R> {
        HttpFrameRead {
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
                return Ok(Async::NotReady)
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
            warn!("closing conn because peer sent frame with size: {}, max_frame_size: {}",
                header.length, max_frame_size);
            return Err(error::Error::CodeError(ErrorCode::FrameSizeError));
        }

        let total_len = FRAME_HEADER_LEN + header.length as usize;

        if let Async::NotReady = self.fill_buff_to_at_least(total_len)? {
            return Ok(Async::NotReady);
        }

        Ok(Async::Ready(RawFrame {
            raw_content: self.buf.split_to(total_len).freeze()
        }))
    }

    fn poll_http_frame(&mut self, max_frame_size: u32) -> Result<Async<HttpFrame>, error::Error> {
        Ok(match self.poll_raw_frame(max_frame_size)? {
            Async::Ready(frame) => Async::Ready(HttpFrame::from_raw(&frame)?),
            Async::NotReady => Async::NotReady,
        })
    }

    pub fn recv_http_frame(self, max_frame_size: u32) -> impl Future<Item=(Self, HttpFrame), Error=error::Error> {
        HttpFrameReadFuture {
            framed_read: Some(self),
            max_frame_size,
        }
    }
}

struct HttpFrameReadFuture<R : AsyncRead> {
    framed_read: Option<HttpFrameRead<R>>,
    max_frame_size: u32,
}

impl<R : AsyncRead> Future for HttpFrameReadFuture<R> {
    type Item = (HttpFrameRead<R>, HttpFrame);
    type Error = error::Error;

    fn poll(&mut self) -> Result<Async<(HttpFrameRead<R>, HttpFrame)>, error::Error> {
        let frame = match self.framed_read.as_mut().unwrap().poll_http_frame(self.max_frame_size)? {
            Async::NotReady => return Ok(Async::NotReady),
            Async::Ready(frame) => frame,
        };

        let framed_read = self.framed_read.take().unwrap();
        Ok(Async::Ready((framed_read, frame)))
    }
}
