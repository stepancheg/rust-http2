#![allow(dead_code)]

use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use futures::sync::mpsc::UnboundedSender;

use stream_part::*;

use solicit::StreamId;
use solicit::DEFAULT_SETTINGS;

use error;

use super::conn::CommonToWriteMessage;
use super::types::Types;
use super::stream_queue_sync::StreamQueueSyncReceiver;


/// Stream that provides data from network.
/// Most importantly, it increases WINDOW.
pub struct StreamFromNetwork<T : Types> {
    pub rx: StreamQueueSyncReceiver,
    pub stream_id: StreamId,
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
    pub in_window_size: u32,
}

impl<T : Types> Stream for StreamFromNetwork<T> {
    type Item = HttpStreamPart;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<HttpStreamPart>, error::Error> {
        let part = match self.rx.poll() {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(e),
            Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(part))) => part,
        };

        if let HttpStreamPart { content: HttpStreamPartContent::Data(ref b), .. } = part {
            self.in_window_size -= b.len() as u32;

            // TODO: use different
            // TODO: increment after process of the frame (i. e. on next poll)
            let edge = DEFAULT_SETTINGS.initial_window_size / 2;
            if self.in_window_size + self.rx.data_size() < edge {
                let inc = DEFAULT_SETTINGS.initial_window_size;
                let m = CommonToWriteMessage::IncreaseInWindow(self.stream_id, inc);
                if let Err(_) = self.to_write_tx.send(m.into()) {
                    return Err(error::Error::Other("failed to send to conn; likely died"));
                }
                self.in_window_size += inc;
            }
        }

        Ok(Async::Ready(Some(part)))
    }
}

impl<T : Types> Drop for StreamFromNetwork<T> {
    fn drop(&mut self) {
        // TODO: reset stream
    }
}
