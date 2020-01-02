use crate::error;
use tokio_io::AsyncWrite;

use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::frame::FrameIR;
use bytes::Buf;
use futures::Async;
use futures::Poll;

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

    pub fn buffer_frame<F: FrameIR>(&mut self, frame: F) {
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
