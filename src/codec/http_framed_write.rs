use futures::future::Future;
use tokio_io::AsyncWrite;
use error;

use solicit::connection::HttpFrame;
use solicit::frame::FrameIR;
use bytes::Bytes;
use futures::Poll;
use futures::Async;
use bytes::Buf;


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

    pub fn buffer_frame(&mut self, frame: HttpFrame) {
        debug!("send {:?}", frame);

        // TODO: reuse memory
        self.buf.extend_from_slice(&frame.serialize_into_vec());
    }

    pub fn poll_flush(&mut self) -> Poll<(), error::Error> {
        Ok(self.write.write_buf(&mut self.buf)?.map(|_: usize| ()))
    }

    pub fn flush_all(self) -> impl Future<Item=Self, Error=error::Error> {
        FlushAllFuture { write: Some(self) }
    }
}

struct FlushAllFuture<W : AsyncWrite> {
    write: Option<HttpFramedWrite<W>>,
}

impl<W : AsyncWrite> Future for FlushAllFuture<W> {
    type Item = HttpFramedWrite<W>;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<HttpFramedWrite<W>, error::Error> {
        loop {
            if self.write.as_mut().unwrap().buf.remaining() == 0 {
                return Ok(Async::Ready(self.write.take().unwrap()));
            }

            if let Async::NotReady = self.write.as_mut().unwrap().poll_flush()? {
                return Ok(Async::NotReady);
            }
        }
    }
}
