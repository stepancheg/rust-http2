use error;
use tokio_io::AsyncWrite;

use bytes::Buf;
use codec::write_buffer::WriteBuffer;
use futures::Async;
use futures::Poll;
use solicit::frame::FrameIR;
use solicit::frame::HttpFrame;

pub struct HttpFramedWrite<W: AsyncWrite> {
    write: W,
    buf: WriteBuffer,
}

impl<W: AsyncWrite> HttpFramedWrite<W> {
    pub fn new(write: W) -> Self {
        HttpFramedWrite {
            write,
            buf: WriteBuffer::new(),
        }
    }

    pub fn data_len(&self) -> usize {
        self.buf.remaining()
    }

    pub fn buffer_frame<F: Into<HttpFrame>>(&mut self, frame: F) {
        let frame: HttpFrame = frame.into();
        debug!("send {:?}", frame);

        frame.serialize_into(&mut self.buf);
    }

    pub fn poll_flush(&mut self) -> Poll<(), error::Error> {
        loop {
            if !self.buf.has_remaining() {
                return Ok(Async::Ready(()));
            }

            if let Async::NotReady = self.write.write_buf(&mut self.buf)? {
                return Ok(Async::NotReady);
            }
        }
    }
}
