use crate::codec::http_framed_write::HttpFramedWrite;
use crate::result;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::GoawayFrame;
use futures::task::Context;
use std::task::Poll;
use tokio::io::AsyncWrite;

pub struct QueuedWrite<W: AsyncWrite + Unpin> {
    framed_write: HttpFramedWrite<W>,
    // GOAWAY frame is added to the queue.
    goaway_queued: bool,
}

impl<W: AsyncWrite + Unpin> QueuedWrite<W> {
    pub fn new(write: W) -> QueuedWrite<W> {
        QueuedWrite {
            framed_write: HttpFramedWrite::new(write),
            goaway_queued: false,
        }
    }

    pub fn queued_bytes_len(&self) -> usize {
        self.framed_write.data_len()
    }

    pub fn _queued_empty(&self) -> bool {
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

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<result::Result<()>> {
        self.framed_write.poll_flush(cx)
    }

    pub fn _goaway_queued(&self) -> bool {
        self.goaway_queued
    }

    pub fn goaway_queued_and_flushed(&self) -> bool {
        self.goaway_queued && self.framed_write.data_len() == 0
    }
}
