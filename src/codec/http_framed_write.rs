use tokio_io::AsyncWrite;
use error;

use solicit::connection::HttpFrame;
use solicit::frame::FrameIR;
use bytes::Bytes;
use futures::Poll;
use bytes::Buf;
use futures::Async;


// TODO: some tests
#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    position: usize, // must be `<= data.len()`
}

impl Buf for Buffer {
    fn remaining(&self) -> usize {
        debug_assert!(self.position <= self.data.len());
        self.data.len() - self.position
    }

    fn bytes(&self) -> &[u8] {
        &self.data[self.position..]
    }

    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.remaining());
        self.position += cnt;
    }
}

impl Buffer {
    pub fn new() -> Buffer {
        Default::default()
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        // Could do something smarter
        self.data.drain(..self.position);
        self.position = 0;
        self.data.extend_from_slice(data);
    }

    pub fn extend_from_vec(&mut self, data: Vec<u8>) {
        // TODO: reuse memory
        self.extend_from_slice(&data);
    }
}


pub struct HttpFramedWrite<W : AsyncWrite> {
    write: W,
    buf: Buffer,
}

impl<W : AsyncWrite> HttpFramedWrite<W> {
    pub fn new(write: W) -> Self {
        HttpFramedWrite {
            write,
            buf: Buffer::new(),
        }
    }

    pub fn buffer(&mut self, buf: Bytes) {
        // TODO: reuse memory
        self.buf.extend_from_slice(&buf);
    }

    pub fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    pub fn buffer_frame<F : Into<HttpFrame>>(&mut self, frame: F) {
        let frame = frame.into();
        debug!("send {:?}", frame);

        self.buf.extend_from_vec(frame.serialize_into_vec());
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
