use common::conn::Conn;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use common::types::Types;
use solicit::StreamId;

use data_or_headers_with_flag::DataOrHeadersWithFlag;

use bytes::Bytes;
use common::conn::ConnStateSnapshot;
use common::conn_read::ConnReadSideCustom;
use common::iteration_exit::IterationExit;
use common::stream::HttpStreamCommand;
use error;
use futures::sync::oneshot;
use futures::task;
use futures::Async;
use futures::Poll;
use result;
use solicit::end_stream::EndStream;
use solicit::frame::continuation::ContinuationFlag;
use solicit::frame::ContinuationFrame;
use solicit::frame::DataFlag;
use solicit::frame::DataFrame;
use solicit::frame::GoawayFrame;
use solicit::frame::HeadersFlag;
use solicit::frame::HeadersFrame;
use solicit::frame::HttpFrame;
use solicit::frame::RstStreamFrame;
use solicit::frame::SettingsFrame;
use std::cmp;
use ErrorCode;
use Headers;

pub trait ConnWriteSideCustom {
    type Types: Types;

    fn process_message(
        &mut self,
        message: <Self::Types as Types>::ToWriteMessage,
    ) -> result::Result<()>;
}

impl<T> Conn<T>
where
    T: Types,
    Self: ConnReadSideCustom<Types = T>,
    Self: ConnWriteSideCustom<Types = T>,
    HttpStreamCommon<T>: HttpStreamData<Types = T>,
{
    fn write_part_data(&mut self, stream_id: StreamId, data: Bytes, end_stream: EndStream) {
        let max_frame_size = self.peer_settings.max_frame_size as usize;

        // if client requested end of stream,
        // we must send at least one frame with end stream flag
        if end_stream == EndStream::Yes && data.len() == 0 {
            let mut frame = DataFrame::with_data(stream_id, Bytes::new());
            frame.set_flag(DataFlag::EndStream);

            debug!("sending frame {:?}", frame);

            self.queued_write.queue(frame);

            return;
        }

        let mut pos = 0;
        while pos < data.len() {
            let end = cmp::min(data.len(), pos + max_frame_size);

            let end_stream_in_frame = if end == data.len() && end_stream == EndStream::Yes {
                EndStream::Yes
            } else {
                EndStream::No
            };

            let mut frame = DataFrame::with_data(stream_id, data.slice(pos, end));
            if end_stream_in_frame == EndStream::Yes {
                frame.set_flag(DataFlag::EndStream);
            }

            debug!("sending frame {:?}", frame);

            self.queued_write.queue(frame);

            pos = end;
        }
    }

    fn write_part_headers(&mut self, stream_id: StreamId, headers: Headers, end_stream: EndStream) {
        let max_frame_size = self.peer_settings.max_frame_size as usize;

        // TODO: avoid allocation
        let headers_fragment = self
            .encoder
            .encode(headers.0.iter().map(|h| (h.name(), h.value())));

        let mut pos = 0;
        while pos < headers_fragment.len() || pos == 0 {
            let end = cmp::min(headers_fragment.len(), pos + max_frame_size);

            let chunk = headers_fragment.slice(pos, end);

            if pos == 0 {
                let mut frame = HeadersFrame::new(chunk, stream_id);
                if end_stream == EndStream::Yes {
                    frame.set_flag(HeadersFlag::EndStream);
                }

                if end == headers_fragment.len() {
                    frame.set_flag(HeadersFlag::EndHeaders);
                }

                debug!("sending frame {:?}", frame);

                self.queued_write.queue(frame);
            } else {
                let mut frame = ContinuationFrame::new(chunk, stream_id);

                if end == headers_fragment.len() {
                    frame.set_flag(ContinuationFlag::EndHeaders);
                }

                debug!("sending frame {:?}", frame);

                self.queued_write.queue(frame);
            }

            pos = end;
        }
    }

    fn write_part_rst(&mut self, stream_id: StreamId, error_code: ErrorCode) {
        let frame = RstStreamFrame::new(stream_id, error_code);

        debug!("sending frame {:?}", frame);

        self.queued_write.queue(frame);
    }

    fn write_part(&mut self, stream_id: StreamId, part: HttpStreamCommand) {
        match part {
            HttpStreamCommand::Data(data, end_stream) => {
                self.write_part_data(stream_id, data, end_stream);
            }
            HttpStreamCommand::Headers(headers, end_stream) => {
                self.write_part_headers(stream_id, headers, end_stream);
            }
            HttpStreamCommand::Rst(error_code) => {
                self.write_part_rst(stream_id, error_code);
            }
        }
    }

    fn has_write_buffer_capacity(&self) -> bool {
        self.queued_write.queued_bytes_len() < 0x8000
    }

    fn pop_outg_for_stream(
        &mut self,
        stream_id: StreamId,
    ) -> Option<(StreamId, HttpStreamCommand, bool)> {
        let stream = self.streams.get_mut(stream_id).unwrap();
        if let (Some(command), stream) = stream.pop_outg_maybe_remove(&mut self.out_window_size) {
            return Some((stream_id, command, stream.is_some()));
        }

        None
    }

    pub fn buffer_outg_conn(&mut self) -> result::Result<bool> {
        let mut updated = false;

        // shortcut
        if !self.has_write_buffer_capacity() {
            return Ok(updated);
        }

        let writable_stream_ids = self.streams.writable_stream_ids();

        for &stream_id in &writable_stream_ids {
            loop {
                if !self.has_write_buffer_capacity() {
                    return Ok(updated);
                }

                if let Some((stream_id, part, cont)) = self.pop_outg_for_stream(stream_id) {
                    self.write_part(stream_id, part);
                    updated = true;

                    // Stream is removed from map, need to continue to the next stream
                    if !cont {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        Ok(updated)
    }

    pub fn send_frame_and_notify<F: Into<HttpFrame>>(&mut self, frame: F) {
        // TODO: some of frames should not be in front of GOAWAY
        self.queued_write.queue(frame);
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

    fn process_stream_end(
        &mut self,
        stream_id: StreamId,
        error_code: ErrorCode,
    ) -> result::Result<()> {
        let stream = self.streams.get_mut(stream_id);
        if let Some(mut stream) = stream {
            stream.close_outgoing(error_code);
        }
        Ok(())
    }

    fn process_stream_enqueue(
        &mut self,
        stream_id: StreamId,
        part: DataOrHeadersWithFlag,
    ) -> result::Result<()> {
        let stream = self.streams.get_mut(stream_id);
        if let Some(mut stream) = stream {
            stream.push_back_part(part);
        }
        Ok(())
    }

    pub fn process_common_message(&mut self, common: CommonToWriteMessage) -> result::Result<()> {
        match common {
            CommonToWriteMessage::StreamEnd(stream_id, error_code) => {
                self.process_stream_end(stream_id, error_code)?;
            }
            CommonToWriteMessage::StreamEnqueue(stream_id, part) => {
                self.process_stream_enqueue(stream_id, part)?;
            }
            CommonToWriteMessage::IncreaseInWindow(stream_id, increase) => {
                self.increase_in_window(stream_id, increase)?;
            }
            CommonToWriteMessage::DumpState(sender) => {
                self.process_dump_state(sender)?;
            }
        }
        Ok(())
    }

    pub fn send_goaway(&mut self, error_code: ErrorCode) -> result::Result<()> {
        debug!("requesting to send GOAWAY with code {:?}", error_code);
        let frame = GoawayFrame::new(self.last_peer_stream_id, error_code);
        self.queued_write.queue_goaway(frame);
        task::current().notify();
        Ok(())
    }

    pub fn process_goaway_state(&mut self) -> result::Result<IterationExit> {
        Ok(if self.queued_write.goaway_queued() {
            self.queued_write.poll()?;
            if self.queued_write.queued_empty() {
                IterationExit::ExitEarly
            } else {
                IterationExit::NotReady
            }
        } else {
            IterationExit::Continue
        })
    }

    fn process_write_queue(&mut self) -> Poll<(), error::Error> {
        loop {
            let message = match self.write_rx.poll()? {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(Some(message)) => message,
                Async::Ready(None) => return Ok(Async::Ready(())), // Add some diagnostics maybe?
            };

            self.process_message(message)?;
        }
    }

    fn poll_flush(&mut self) -> result::Result<()> {
        self.buffer_outg_conn()?;
        loop {
            self.queued_write.poll()?;
            let updated = self.buffer_outg_conn()?;
            if !updated {
                return Ok(());
            }
        }
    }

    pub fn poll_write(&mut self) -> Poll<(), error::Error> {
        if let Async::Ready(()) = self.process_write_queue()? {
            return Ok(Async::Ready(()));
        }

        self.poll_flush()?;

        Ok(Async::NotReady)
    }
}

// Message sent to write loop.
// Processed while write loop is not handling network I/O.
pub enum CommonToWriteMessage {
    IncreaseInWindow(StreamId, u32),
    StreamEnqueue(StreamId, DataOrHeadersWithFlag),
    StreamEnd(StreamId, ErrorCode), // send when user provided handler completed the stream
    DumpState(oneshot::Sender<ConnStateSnapshot>),
}
