use common::types::Types;
use common::conn::Conn;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use solicit::connection::HttpFrame;
use solicit::StreamId;

use data_or_headers_with_flag::DataOrHeadersWithFlag;

use error;
use ErrorCode;
use solicit::frame::RstStreamFrame;
use solicit::frame::GoawayFrame;
use solicit::frame::WindowUpdateFrame;
use solicit::frame::SettingsFrame;
use result;
use futures::Poll;
use futures::Async;
use common::conn_read::ConnReadSideCustom;
use common::conn_command::ConnCommandSideCustom;
use common::stream::HttpStreamCommand;
use solicit::frame::FrameBuilder;
use std::cmp;
use solicit::frame::HeadersFrame;
use solicit::connection::EndStream;
use solicit::frame::HeadersFlag;
use solicit::frame::ContinuationFrame;
use solicit::frame::continuation::ContinuationFlag;
use solicit::frame::DataFrame;
use solicit::frame::DataFlag;
use solicit::frame::FrameIR;
use std::mem;
use common::goaway_state::GoAwayState;
use futures::task;


pub enum DirectlyToNetworkFrame {
    WindowUpdate(WindowUpdateFrame),
}

impl DirectlyToNetworkFrame {
    pub fn into_http_frame(self) -> HttpFrame {
        match self {
            DirectlyToNetworkFrame::WindowUpdate(f) => f.into(),
        }
    }
}


pub trait ConnWriteSideCustom {
    type Types : Types;

    fn process_message(&mut self, message: <Self::Types as Types>::ToWriteMessage) -> result::Result<()>;
}

impl<T> Conn<T>
    where
        T : Types,
        Self : ConnReadSideCustom<Types=T>,
        Self : ConnWriteSideCustom<Types=T>,
        Self : ConnCommandSideCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    fn poll_flush(&mut self) -> Poll<(), error::Error> {
        self.framed_write.poll_flush()
    }

    fn write_part(&mut self, target: &mut FrameBuilder, stream_id: StreamId, part: HttpStreamCommand) {
        let max_frame_size = self.peer_settings.max_frame_size as usize;

        match part {
            HttpStreamCommand::Data(data, end_stream) => {
                // if client requested end of stream,
                // we must send at least one frame with end stream flag
                if end_stream == EndStream::Yes && data.len() == 0 {
                    // probably should send RST_STREAM
                    let mut frame = DataFrame::with_data(stream_id, Vec::new());
                    frame.set_flag(DataFlag::EndStream);

                    debug!("sending frame {:?}", frame);

                    frame.serialize_into(target);
                    return;
                }

                let mut pos = 0;
                while pos < data.len() {
                    let end = cmp::min(data.len(), pos + max_frame_size);

                    let end_stream_in_frame =
                        if end == data.len() && end_stream == EndStream::Yes {
                            EndStream::Yes
                        } else {
                            EndStream::No
                        };

                    let mut frame = DataFrame::with_data(stream_id, data.slice(pos, end));
                    if end_stream_in_frame == EndStream::Yes {
                        frame.set_flag(DataFlag::EndStream);
                    }

                    debug!("sending frame {:?}", frame);

                    frame.serialize_into(target);

                    pos = end;
                }
            }
            HttpStreamCommand::Headers(headers, end_stream) => {
                let headers_fragment = self
                    .encoder.encode(headers.0.iter().map(|h| (h.name(), h.value())));

                let mut pos = 0;
                while pos < headers_fragment.len() || pos == 0 {
                    let end = cmp::min(headers_fragment.len(), pos + max_frame_size);

                    let chunk = &headers_fragment[pos..end];

                    if pos == 0 {
                        let mut frame = HeadersFrame::new(chunk, stream_id);
                        if end_stream == EndStream::Yes {
                            frame.set_flag(HeadersFlag::EndStream);
                        }

                        if end == headers_fragment.len() {
                            frame.set_flag(HeadersFlag::EndHeaders);
                        }

                        debug!("sending frame {:?}", frame);

                        frame.serialize_into(target);
                    } else {
                        let mut frame = ContinuationFrame::new(chunk, stream_id);

                        if end == headers_fragment.len() {
                            frame.set_flag(ContinuationFlag::EndHeaders);
                        }

                        debug!("sending frame {:?}", frame);

                        frame.serialize_into(target);
                    }

                    pos = end;
                }
            }
            HttpStreamCommand::Rst(error_code) => {
                let frame = RstStreamFrame::new(stream_id, error_code);

                debug!("sending frame {:?}", frame);

                frame.serialize_into(target);
            }
        }
    }

    fn pop_outg_all_for_stream_bytes(&mut self, stream_id: StreamId) -> Vec<u8> {
        let mut send = FrameBuilder::new();
        for part in self.pop_outg_all_for_stream(stream_id) {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    fn pop_outg_all_for_conn_bytes(&mut self) -> Vec<u8> {
        // TODO: maintain own limits of out window
        let mut send = FrameBuilder::new();
        for (stream_id, part) in self.pop_outg_all_for_conn() {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    fn pop_outg_all_for_stream(&mut self, stream_id: StreamId) -> Vec<HttpStreamCommand> {
        if let Some(stream) = self.streams.get_mut(stream_id) {
            stream.pop_outg_all_maybe_remove(&mut self.out_window_size)
        } else {
            Vec::new()
        }
    }

    pub fn pop_outg_all_for_conn(&mut self) -> Vec<(StreamId, HttpStreamCommand)> {
        let mut r = Vec::new();

        // TODO: keep list of streams with data
        for stream_id in self.streams.stream_ids() {
            r.extend(self.pop_outg_all_for_stream(stream_id).into_iter().map(|s| (stream_id, s)));
        }

        r
    }

    pub fn buffer_outg_stream(&mut self, stream_id: StreamId) -> result::Result<()> {
        let bytes = self.pop_outg_all_for_stream_bytes(stream_id);

        self.framed_write.buffer(bytes.into());

        Ok(())
    }

    fn buffer_outg_conn(&mut self) -> result::Result<()> {
        let bytes = self.pop_outg_all_for_conn_bytes();

        self.framed_write.buffer(bytes.into());

        Ok(())
    }

    pub fn send_frame_and_notify<F : Into<HttpFrame>>(&mut self, frame: F) {
        // TODO: some of frames should not be in front of GOAWAY
        self.framed_write.buffer_frame(frame);
        // Notify the task to make sure write loop is called again
        // to flush the buffer
        task::current().notify();
    }

    /// Sends an SETTINGS Frame with ack set to acknowledge seeing a SETTINGS frame from the peer.
    pub fn send_ack_settings(&mut self) -> result::Result<()> {
        let settings = SettingsFrame::new_ack();
        self.send_frame_and_notify(settings);
        Ok(())
    }

    fn process_stream_end(&mut self, stream_id: StreamId, error_code: ErrorCode) -> result::Result<()> {
        let stream_id = {
            let stream = self.streams.get_mut(stream_id);
            if let Some(mut stream) = stream {
                stream.stream().outgoing.close(error_code);
                Some(stream_id)
            } else {
                None
            }
        };
        if let Some(stream_id) = stream_id {
            self.buffer_outg_stream(stream_id)?;
        }
        Ok(())
    }

    fn process_stream_enqueue(&mut self, stream_id: StreamId, part: DataOrHeadersWithFlag) -> result::Result<()> {
        let stream_id = {
            let stream = self.streams.get_mut(stream_id);
            if let Some(mut stream) = stream {
                stream.stream().outgoing.push_back_part(part);
                Some(stream_id)
            } else {
                None
            }
        };
        if let Some(stream_id) = stream_id {
            self.buffer_outg_stream(stream_id)?;
        }
        Ok(())
    }

    pub fn process_common_message(&mut self, common: CommonToWriteMessage) -> result::Result<()> {
        match common {
            CommonToWriteMessage::Frame(frame) => {
                self.framed_write.buffer_frame(frame.into_http_frame());
            },
            CommonToWriteMessage::StreamEnd(stream_id, error_code) => {
                self.process_stream_end(stream_id, error_code)?;
            },
            CommonToWriteMessage::StreamEnqueue(stream_id, part) => {
                self.process_stream_enqueue(stream_id, part)?;
            },
            CommonToWriteMessage::IncreaseInWindow(stream_id, increase) => {
                self.increase_in_window(stream_id, increase)?;
            },
        }
        Ok(())
    }

    fn process_flush_xxx_fields(&mut self) -> result::Result<()> {
        if mem::replace(&mut self.flush_conn, false) {
            self.buffer_outg_conn()?;
        }

        let stream_ids: Vec<StreamId> = self.flush_streams.iter().cloned().collect();
        self.flush_streams.clear();
        for stream_id in stream_ids {
            self.buffer_outg_stream(stream_id)?;
        }

        Ok(())
    }

    fn process_goaway_state(&mut self) -> result::Result<()> {
        loop {
            match self.goaway_state {
                GoAwayState::None => return Ok(()),
                GoAwayState::NeedToSend(error_code) => {
                    let frame = GoawayFrame::new(self.last_peer_stream_id, error_code);
                    self.framed_write.buffer_frame(HttpFrame::Goaway(frame));
                    self.goaway_state = GoAwayState::Sending;
                    continue;
                }
                GoAwayState::Sending => {
                    match self.poll_flush()? {
                        Async::Ready(()) => {
                            self.goaway_state = GoAwayState::Sent;
                        },
                        Async::NotReady => {}
                    }
                    return Ok(());
                }
                GoAwayState::Sent => return Ok(()),
            }
        }
    }

    pub fn poll_write(&mut self) -> Poll<(), error::Error> {
        loop {
            self.process_goaway_state()?;

            match self.goaway_state {
                GoAwayState::None => {}
                GoAwayState::NeedToSend(..) => {
                    unreachable!();
                }
                GoAwayState::Sending => {
                    assert!(self.framed_write.remaining() != 0);
                    return Ok(Async::NotReady);
                }
                GoAwayState::Sent => {
                    info!("GOAWAY sent, exiting loop");
                    assert!(self.framed_write.remaining() == 0);
                    return Ok(Async::Ready(()));
                }
            }

            self.process_flush_xxx_fields()?;

            if let Async::NotReady = self.poll_flush()? {
                return Ok(Async::NotReady);
            }

            let message = match self.write_rx.poll()? {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(Some(message)) => message,
                Async::Ready(None) => return Ok(Async::Ready(())), // Add some diagnostics maybe?
            };

            self.process_message(message)?;
        }
    }
}


// Message sent to write loop.
// Processed while write loop is not handling network I/O.
pub enum CommonToWriteMessage {
    IncreaseInWindow(StreamId, u32),
    Frame(DirectlyToNetworkFrame),    // write frame immediately to the network
    StreamEnqueue(StreamId, DataOrHeadersWithFlag),
    StreamEnd(StreamId, ErrorCode),   // send when user provided handler completed the stream
}
