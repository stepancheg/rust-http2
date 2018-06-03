use tokio_io::AsyncWrite;
use codec::http_framed_write::HttpFramedWrite;
use solicit::connection::HttpFrame;
use std::collections::VecDeque;
use futures::Poll;
use error;
use futures::Async;
use bytes::Bytes;
use solicit::frame::GoawayFrame;


enum HttpFrameOrBytes {
    Frame(HttpFrame),
    Bytes(Bytes),
}


pub struct QueuedWrite<W : AsyncWrite> {
    framed_write: HttpFramedWrite<W>,
    frames: VecDeque<HttpFrameOrBytes>,
    goaway_queued: bool,
}

impl<W : AsyncWrite> QueuedWrite<W> {
    pub fn new(write: W) -> QueuedWrite<W> {
        QueuedWrite {
            framed_write: HttpFramedWrite::new(write),
            frames: VecDeque::new(),
            goaway_queued: false,
        }
    }

    fn queue_frame_or_bytes(&mut self, frame: HttpFrameOrBytes) {
        if self.goaway_queued {
            return;
        }

        self.frames.push_back(frame);
    }

    pub fn queue<F : Into<HttpFrame>>(&mut self, frame: F) {
        self.queue_frame_or_bytes(HttpFrameOrBytes::Frame(frame.into()));
    }

    pub fn queue_bytes<B : Into<Bytes>>(&mut self, frame: B) {
        self.queue_frame_or_bytes(HttpFrameOrBytes::Bytes(frame.into()));
    }

    pub fn queue_goaway(&mut self, frame: GoawayFrame) {
        // If we decided to terminate, send goaway immediately
        // and discard queued frames
        if self.goaway_queued {
            return;
        }
        self.goaway_queued = true;
        self.frames.clear();

        self.framed_write.buffer_frame(frame);
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

    pub fn goaway_queued(&self) -> bool {
        self.goaway_queued
    }
}
