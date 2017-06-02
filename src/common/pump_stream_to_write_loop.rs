use futures::Async;
use futures::Poll;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::UnboundedSender;

use solicit::StreamId;

use futures_misc::latch;

use rc_mut::RcMut;

use stream_part::HttpPartStream;

use error::ErrorCode;

use super::*;

/// Poll the stream and enqueues frames
pub struct PumpStreamToWriteLoop<T : Types> {
    conn_rc: RcMut<ConnData<T>>,
    to_write_tx: UnboundedSender<T::ToWriteMessage>,
    stream_id: StreamId,
    ready_to_write: latch::Latch,
    stream: HttpPartStream,
}

impl<T : Types> PumpStreamToWriteLoop<T> {
    pub fn new(
        conn_rc: RcMut<ConnData<T>>,
        to_write_tx: UnboundedSender<T::ToWriteMessage>,
        stream_id: StreamId,
        ready_to_write: latch::Latch,
        stream: HttpPartStream)
            -> PumpStreamToWriteLoop<T>
    {
        PumpStreamToWriteLoop {
            conn_rc: conn_rc,
            to_write_tx: to_write_tx,
            stream_id: stream_id,
            ready_to_write: ready_to_write,
            stream: stream,
        }
    }
}

impl<T : Types> Future for PumpStreamToWriteLoop<T> {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.ready_to_write.poll_ready() {
                Err(latch::ControllerDead) => {
                    warn!("error from latch; stream must be closed");
                    break;
                }
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(())) => {}
            }

            let part_opt = match self.stream.poll() {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(r)) => r,
                Err(e) => {
                    warn!("stream error: {:?}", e);
                    let stream_end = CommonToWriteMessage::StreamEnd(self.stream_id, ErrorCode::InternalError);
                    if let Err(e) = self.to_write_tx.send(stream_end.into()) {
                        warn!("failed to write to channel, probably connection is closed: {:?}", e);
                    }
                    break;
                },
            };

            let mut conn = self.conn_rc.borrow_mut();
            let conn: &mut ConnData<T> = &mut conn;

            if let Some(mut stream) = conn.streams.get_mut(self.stream_id) {
                if stream.stream().state.is_closed_local() {
                    break;
                }

                let exit = match part_opt {
                    Some(part) => {
                        stream.stream().outgoing.push_back_part(part);
                        stream.check_ready_to_write(&mut conn.conn.out_window_size);
                        false
                    }
                    None => {
                        stream.stream().outgoing.close(ErrorCode::NoError);
                        true
                    }
                };

                let flush_stream = CommonToWriteMessage::TryFlushStream(Some(self.stream_id));
                if let Err(e) = self.to_write_tx.send(flush_stream.into()) {
                    warn!("failed to write to channel, probably connection is closed: {:?}", e);
                }

                if exit {
                    break;
                } else {
                    continue;
                }
            } else {
                break;
            }
        }

        Ok(Async::Ready(()))
    }
}
