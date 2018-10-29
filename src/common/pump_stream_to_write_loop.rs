use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::UnboundedSender;
use futures::Async;
use futures::Poll;

use void::Void;

use solicit::StreamId;

use error::ErrorCode;

use super::*;
use misc::any_to_string;
use std::panic;
use std::panic::AssertUnwindSafe;
use DataOrTrailers;
use HttpStreamAfterHeaders;

/// Poll the stream and enqueues frames
pub struct PumpStreamToWrite<T: Types> {
    // TODO: this is not thread-safe
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
    pub stream_id: StreamId,
    pub out_window: window_size::StreamOutWindowReceiver,
    pub stream: HttpStreamAfterHeaders,
}

impl<T: Types> Future for PumpStreamToWrite<T> {
    type Item = ();
    type Error = Void;

    fn poll(&mut self) -> Poll<(), Void> {
        loop {
            // Note poll returns Ready when window size is > 0,
            // although HEADERS could be sent even when window size is zero or negative.
            match self.out_window.poll() {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(())) => {}
                Err(window_size::StreamDead::Conn) => {
                    warn!("conn dead");
                    return Ok(Async::Ready(()));
                }
                Err(window_size::StreamDead::Stream) => {
                    warn!("stream {} dead", self.stream_id);
                    return Ok(Async::Ready(()));
                }
            }

            let poll = match panic::catch_unwind(AssertUnwindSafe(|| self.stream.poll())) {
                Ok(poll) => poll,
                Err(e) => {
                    let e = any_to_string(e);
                    warn!("stream panicked: {}", e);
                    let rst =
                        CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    drop(self.to_write_tx.unbounded_send(rst.into()));
                    return Ok(Async::Ready(()));
                }
            };

            let part_opt = match poll {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(r)) => r,
                Err(e) => {
                    warn!("stream error: {:?}", e);
                    let stream_end =
                        CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    if let Err(e) = self.to_write_tx.unbounded_send(stream_end.into()) {
                        warn!(
                            "failed to write to channel, probably connection is closed: {:?}",
                            e
                        );
                    }
                    break;
                }
            };

            match part_opt {
                Some(part) => {
                    if let DataOrTrailers::Data(ref d, _) = part {
                        self.out_window.decrease(d.len());
                    }

                    let msg = CommonToWriteMessage::StreamEnqueue(self.stream_id, part.into());
                    if let Err(e) = self.to_write_tx.unbounded_send(msg.into()) {
                        warn!(
                            "failed to write to channel, probably connection is closed: {:?}",
                            e
                        );
                        break;
                    }

                    continue;
                }
                None => {
                    let msg = CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::NoError);
                    if let Err(e) = self.to_write_tx.unbounded_send(msg.into()) {
                        warn!(
                            "failed to write to channel, probably connection is closed: {:?}",
                            e
                        );
                        break;
                    }

                    break;
                }
            }
        }

        Ok(Async::Ready(()))
    }
}
