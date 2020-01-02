use std::future::Future;
use std::task::Poll;

use super::*;
use crate::common::conn_command_channel::ConnCommandSender;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::types::Types;
use crate::misc::any_to_string;
use crate::solicit::stream_id::StreamId;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::HttpStreamAfterHeaders;
use futures::stream::Stream;
use futures::task::Context;
use std::panic;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;

/// Poll the stream and enqueues frames
pub(crate) struct PumpStreamToWrite<T: Types> {
    // TODO: this is not thread-safe
    pub to_write_tx: ConnCommandSender<T>,
    pub stream_id: StreamId,
    pub out_window: window_size::StreamOutWindowReceiver,
    pub stream: HttpStreamAfterHeaders,
}

impl<T: Types> Future for PumpStreamToWrite<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            // Note poll returns Ready when window size is > 0,
            // although HEADERS could be sent even when window size is zero or negative.
            match self.out_window.poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(window_size::StreamDead::Conn)) => {
                    warn!("conn dead");
                    return Poll::Ready(());
                }
                Poll::Ready(Err(window_size::StreamDead::Stream)) => {
                    warn!("stream {} dead", self.stream_id);
                    return Poll::Ready(());
                }
            }

            let poll = match panic::catch_unwind(AssertUnwindSafe(|| {
                Pin::new(&mut self.stream).poll_next(cx)
            })) {
                Ok(poll) => poll,
                Err(e) => {
                    let e = any_to_string(e);
                    warn!("stream panicked: {}", e);
                    let rst =
                        CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    drop(self.to_write_tx.unbounded_send(rst.into()));
                    return Poll::Ready(());
                }
            };

            let part_opt = match poll {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => None,
                Poll::Ready(Some(Ok(r))) => Some(r),
                Poll::Ready(Some(Err(e))) => {
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

        Poll::Ready(())
    }
}
