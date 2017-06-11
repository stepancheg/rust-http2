use futures::Async;
use futures::Poll;
use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::UnboundedSender;

use void::Void;

use solicit::StreamId;

use rc_mut::RcMut;

use stream_part::HttpPartStream;
use stream_part::HttpStreamPartContent;

use error::ErrorCode;

use super::*;

/// Poll the stream and enqueues frames
pub struct PumpStreamToWriteLoop<T : Types> {
    // TODO: this is not thread-safe
    pub conn_rc: RcMut<ConnData<T>>,
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
    pub stream_id: StreamId,
    pub out_window: window_size::StreamOutWindowReceiver,
    pub stream: HttpPartStream,
}

//unsafe impl <T : Types> Send for PumpStreamToWriteLoop<T> {}
//unsafe impl <T : Types> Sync for PumpStreamToWriteLoop<T> {}

impl<T : Types> Future for PumpStreamToWriteLoop<T> {
    type Item = ();
    type Error = Void;

    fn poll(&mut self) -> Poll<(), Void> {
        loop {
            match self.out_window.poll() {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(()) => {}
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
                        match &part.content {
                            &HttpStreamPartContent::Data(ref d) => {
                                self.out_window.decrease(d.len() as u32); // TODO: check overflow
                            }
                            &HttpStreamPartContent::Headers(_) => {
                            }
                        }

                        stream.stream().outgoing.push_back_part(part);

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
