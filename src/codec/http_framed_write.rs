use tokio_io::AsyncWrite;
use error;

use solicit::connection::HttpFrame;
use solicit::frame::FrameIR;
use bytes::Bytes;
use futures::Poll;
use bytes::Buf;
use futures::Async;
use codec::write_buffer::WriteBuffer;


pub struct HttpFramedWrite<W : AsyncWrite> {
    write: W,
    buf: WriteBuffer,
}

impl<W : AsyncWrite> HttpFramedWrite<W> {
    pub fn new(write: W) -> Self {
        HttpFramedWrite {
            write,
            buf: WriteBuffer::new(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    pub fn buffer_frame<F : Into<HttpFrame>>(&mut self, frame: F) {
        let frame = frame.into();
        debug!("send {:?}", frame);

        self.buf.extend_from_vec(frame.serialize_into_vec());
    }

    pub fn buffer_bytes<B : Into<Bytes>>(&mut self, bytes: B) {
        let bytes = bytes.into();
        self.buf.extend_from_bytes(bytes);
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

