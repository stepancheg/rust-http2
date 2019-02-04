use futures::future::Future;
use futures::stream::Stream;
use futures::Async;
use futures::Poll;

use void::Void;

use super::*;
use common::conn_command_channel::ConnCommandSender;
use common::conn_write::CommonToWriteMessage;
use common::types::Types;
use misc::any_to_string;
use solicit::stream_id::StreamId;
use std::panic;
use std::panic::AssertUnwindSafe;
use DataOrTrailers;
use ErrorCode;
use HttpStreamAfterHeaders;

/// Poll the stream and enqueues frames
pub(crate) struct PumpStreamToWrite<T: Types> {
    // TODO: this is not thread-safe
    pub to_write_tx: ConnCommandSender<T>,
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
                    ndc_warn!("conn dead");
                    return Ok(Async::Ready(()));
                }
                Err(window_size::StreamDead::Stream) => {
                    ndc_warn!("stream {} dead", self.stream_id);
                    return Ok(Async::Ready(()));
                }
            }

            let poll = match panic::catch_unwind(AssertUnwindSafe(|| self.stream.poll())) {
                Ok(poll) => poll,
                Err(e) => {
                    let e = any_to_string(e);
                    ndc_warn!("stream panicked: {}", e);
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
                    ndc_warn!("stream error: {:?}", e);
                    let stream_end =
                        CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    if let Err(e) = self.to_write_tx.unbounded_send(stream_end.into()) {
                        ndc_warn!(
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
                        ndc_warn!(
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
                        ndc_warn!(
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
