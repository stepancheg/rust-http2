use crate::result;
use tokio::io::AsyncWrite;

use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::frame::FrameIR;
use bytes::Buf;
use futures::task::Context;
use std::pin::Pin;
use std::task::Poll;

pub struct HttpFramedWrite<W: AsyncWrite + Unpin> {
    write: W,
    buf: WriteBuffer,
}

impl<W: AsyncWrite + Unpin> HttpFramedWrite<W> {
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

    pub fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<result::Result<()>> {
        loop {
            if !self.buf.has_remaining() {
                return Poll::Ready(Ok(()));
            }

            if let Poll::Pending =
                tokio_util::io::poll_write_buf(Pin::new(&mut self.write), cx, &mut self.buf)?
            {
                return Poll::Pending;
            }
        }
    }
}
