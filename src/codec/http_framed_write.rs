use error;
use tokio_io::AsyncWrite;

use bytes::Buf;
use codec::write_buffer::WriteBuffer;
use futures::Async;
use futures::Poll;
use solicit::connection::HttpFrame;
use solicit::frame::FrameIR;

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

    pub fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    pub fn buffer_frame<F: Into<HttpFrame>>(&mut self, frame: F) {
        let frame: HttpFrame = frame.into();
        debug!("send {:?}", frame);

        frame.serialize_into(&mut self.buf);
    }

    pub fn poll_flush(&mut self) -> Poll<(), error::Error> {
        loop {
            if self.buf.remaining() == 0 {
                return Ok(Async::Ready(()));
            }

            if let Async::NotReady = self.write.write_buf(&mut self.buf)? {
                return Ok(Async::NotReady);
            }
        }
    }
}
