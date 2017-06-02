#![allow(dead_code)]

use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use futures::sync::mpsc::UnboundedSender;

use stream_part::*;

use futures_misc::signal;

use solicit::StreamId;

use error;

use super::conn::CommonToWriteMessage;
use super::types::Types;
use super::stream_queue_sync::StreamQueueSyncReceiver;


pub struct StreamFromNetwork<T : Types> {
    signal: signal::Receiver,
    rx: StreamQueueSyncReceiver,
    stream_id: StreamId,
    to_write_tx: UnboundedSender<T::ToWriteMessage>,
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

        if let HttpStreamPart { content: HttpStreamPartContent::Data(_), .. } = part {
            let increase_window = CommonToWriteMessage::IncreaseInWindow(self.stream_id);
            if let Err(_) = self.to_write_tx.send(increase_window.into()) {
                return Err(error::Error::Other("failed to send to conn; likely died"));
            }
        }

        Ok(Async::Ready(Some(part)))
    }
}
