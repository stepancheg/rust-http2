use tokio_io::AsyncWrite;
use codec::http_framed_write::HttpFramedWrite;
use solicit::connection::HttpFrame;
use std::collections::VecDeque;
use futures::Poll;
use error;
use futures::Async;
use bytes::Bytes;


enum HttpFrameOrBytes {
    Frame(HttpFrame),
    Bytes(Bytes),
}


pub struct QueuedWrite<W : AsyncWrite> {
    framed_write: HttpFramedWrite<W>,
    frames: VecDeque<HttpFrameOrBytes>,
}

impl<W : AsyncWrite> QueuedWrite<W> {
    pub fn new(write: W) -> QueuedWrite<W> {
        QueuedWrite {
            framed_write: HttpFramedWrite::new(write),
            frames: VecDeque::new(),
        }
    }

    pub fn queue<F : Into<HttpFrame>>(&mut self, frame: F) {
        self.frames.push_back(HttpFrameOrBytes::Frame(frame.into()));
    }

    pub fn queue_bytes<B : Into<Bytes>>(&mut self, frame: B) {
        self.frames.push_back(HttpFrameOrBytes::Bytes(frame.into()));
    }

    pub fn poll(&mut self) -> Poll<(), error::Error> {
        loop {
            match self.framed_write.poll_flush()? {
                Async::Ready(()) => {}
                Async::NotReady => {
                    return Ok(Async::NotReady)
                }
            }

            match self.frames.pop_front() {
                Some(HttpFrameOrBytes::Frame(frame)) => self.framed_write.buffer_frame(frame),
                Some(HttpFrameOrBytes::Bytes(bytes)) => self.framed_write.buffer_bytes(bytes),
                None => return Ok(Async::Ready(())),
            }
        }
    }

    pub fn remaining_empty(&self) -> bool {
        self.framed_write.remaining() == 0 && self.frames.is_empty()
    }
}
