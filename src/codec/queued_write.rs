use codec::http_framed_write::HttpFramedWrite;
use error;
use futures::Poll;
use solicit::connection::HttpFrame;
use solicit::connection::HttpFrameType;
use solicit::frame::GoawayFrame;
use tokio_io::AsyncWrite;

pub struct QueuedWrite<W: AsyncWrite> {
    framed_write: HttpFramedWrite<W>,
    // GOAWAY frame is added to the queue.
    goaway_queued: bool,
}

impl<W: AsyncWrite> QueuedWrite<W> {
    pub fn new(write: W) -> QueuedWrite<W> {
        QueuedWrite {
            framed_write: HttpFramedWrite::new(write),
            goaway_queued: false,
        }
    }

    pub fn remaining(&self) -> usize {
        self.framed_write.remaining()
    }

    pub fn queue<F: Into<HttpFrame>>(&mut self, frame: F) {
        if self.goaway_queued {
            return;
        }

        let frame = frame.into();

        debug_assert!(frame.frame_type() != HttpFrameType::Goaway);
        self.framed_write.buffer_frame(frame)
    }

    pub fn queue_goaway(&mut self, frame: GoawayFrame) {
        // If we decided to terminate, send goaway immediately
        // and discard queued frames
        if self.goaway_queued {
            return;
        }
        self.goaway_queued = true;

        self.framed_write.buffer_frame(frame);
    }

    pub fn poll(&mut self) -> Poll<(), error::Error> {
        self.framed_write.poll_flush()
    }

    pub fn remaining_empty(&self) -> bool {
        self.framed_write.remaining() == 0
    }

    pub fn goaway_queued(&self) -> bool {
        self.goaway_queued
    }
}
