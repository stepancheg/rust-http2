use futures::future::FutureExt;
use futures::stream::StreamExt;

use super::*;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::death_aware_channel::DeathAwareSender;
use crate::common::types::Types;
use crate::misc::any_to_string;
use crate::solicit::stream_id::StreamId;
use crate::DataOrTrailers;

use crate::ErrorCode;
use crate::HttpStreamAfterHeaders;

use std::panic::AssertUnwindSafe;

/// Poll the stream and enqueues frames
pub(crate) struct PumpStreamToWrite<T: Types> {
    // TODO: this is not thread-safe
    pub to_write_tx: DeathAwareSender<T::ToWriteMessage>,
    pub stream_id: StreamId,
    pub out_window: window_size::StreamOutWindowReceiver,
    pub stream: HttpStreamAfterHeaders,
}

impl<T: Types> PumpStreamToWrite<T> {
    pub async fn run(mut self) {
        loop {
            // Note poll returns Ready when window size is > 0,
            // although HEADERS could be sent even when window size is zero or negative.
            match self.out_window.poll_f().await {
                Ok(()) => {}
                Err(window_size::StreamDead::Conn) => {
                    warn!("conn dead");
                    break;
                }
                Err(window_size::StreamDead::Stream) => {
                    warn!("stream {} dead", self.stream_id);
                    break;
                }
            }

            let poll = match AssertUnwindSafe(self.stream.next()).catch_unwind().await {
                Ok(poll) => poll,
                Err(e) => {
                    let e = any_to_string(e);
                    warn!("stream panicked: {}", e);
                    let rst =
                        CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    drop(self.to_write_tx.unbounded_send(rst.into()));
                    break;
                }
            };

            let part_opt = match poll {
                None => None,
                Some(Ok(r)) => Some(r),
                Some(Err(e)) => {
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
    }
}
