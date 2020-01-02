use crate::codec::http_framed_write::HttpFramedWrite;
use crate::error;
use futures::Poll;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::GoawayFrame;
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

    pub fn queued_bytes_len(&self) -> usize {
        self.framed_write.data_len()
    }

    pub fn queued_empty(&self) -> bool {
        self.queued_bytes_len() == 0
    }

    pub fn queue_not_goaway<F: FrameIR>(&mut self, frame: F) {
        if self.goaway_queued {
            return;
        }

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

    pub fn goaway_queued(&self) -> bool {
        self.goaway_queued
    }
}
