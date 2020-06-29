use crate::common::conn::Conn;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::types::Types;

use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;

use crate::common::conn::ConnStateSnapshot;
use crate::common::conn_read::ConnReadSideCustom;
use crate::common::pump_stream_to_write_loop::PumpStreamToWrite;
use crate::common::stream::HttpStreamCommand;
use crate::common::window_size::StreamOutWindowReceiver;
use crate::data_or_headers::DataOrHeaders;

use crate::result;
use crate::solicit::end_stream::EndStream;
use crate::solicit::frame::DataFlag;
use crate::solicit::frame::DataFrame;
use crate::solicit::frame::Flags;
use crate::solicit::frame::GoawayFrame;
use crate::solicit::frame::HeadersFlag;
use crate::solicit::frame::HeadersMultiFrame;
use crate::solicit::frame::HttpFrame;
use crate::solicit::frame::RstStreamFrame;
use crate::solicit::frame::SettingsFrame;
use crate::solicit::stream_id::StreamId;
use crate::ErrorCode;
use crate::Headers;
use crate::HttpStreamAfterHeaders;
use bytes::Bytes;
use futures::channel::oneshot;
use futures::task::Context;
use std::cmp;

use crate::net::socket::SocketStream;
use std::task::Poll;

pub(crate) trait ConnWriteSideCustom {
    type Types: Types;

    fn process_message(
        &mut self,
        message: <Self::Types as Types>::ToWriteMessage,
    ) -> result::Result<()>;
}

impl<T, I> Conn<T, I>
where
    T: Types,
    Self: ConnReadSideCustom<Types = T>,
    Self: ConnWriteSideCustom<Types = T>,
    HttpStreamCommon<T>: HttpStreamData<Types = T>,
    I: SocketStream,
{
    fn write_part_data(&mut self, stream_id: StreamId, data: Bytes, end_stream: EndStream) {
        let max_frame_size = self.peer_settings.max_frame_size as usize;

        // if client requested end of stream,
        // we must send at least one frame with end stream flag
        if end_stream == EndStream::Yes && data.len() == 0 {
            let mut frame = DataFrame::with_data(stream_id, Bytes::new());
            frame.set_flag(DataFlag::EndStream);

            debug!("sending frame {:?}", frame);

            self.queued_write.queue_not_goaway(frame);

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

            let mut frame = DataFrame::with_data(stream_id, data.slice(pos..end));
            if end_stream_in_frame == EndStream::Yes {
                frame.set_flag(DataFlag::EndStream);
            }

            self.queued_write.queue_not_goaway(frame);

            pos = end;
        }
    }

    fn write_part_headers(&mut self, stream_id: StreamId, headers: Headers, end_stream: EndStream) {
        let mut flags = Flags::new(0);
        if end_stream == EndStream::Yes {
            flags.set(HeadersFlag::EndStream);
        }
        self.queued_write.queue_not_goaway(HeadersMultiFrame {
            flags,
            stream_id,
            headers,
            stream_dep: None,
            padding_len: 0,
            encoder: &mut self.encoder,
            max_frame_size: self.peer_settings.max_frame_size,
        });
    }

    fn write_part_rst(&mut self, stream_id: StreamId, error_code: ErrorCode) {
        let frame = RstStreamFrame::new(stream_id, error_code);

        self.queued_write.queue_not_goaway(frame);
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
        self.queued_write.queue_not_goaway(frame.into());
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
        } else {
            if let DataOrHeaders::Data(data) = part.content {
                self.pump_out_window_size.increase(data.len());
            }
        }
        Ok(())
    }

    fn process_stream_pull(
        &mut self,
        stream_id: StreamId,
        stream: HttpStreamAfterHeaders,
        out_window: StreamOutWindowReceiver,
    ) -> result::Result<()> {
        // TODO: spawn in handler
        self.loop_handle.spawn(
            PumpStreamToWrite::<T> {
                to_write_tx: self.to_write_tx.clone(),
                stream_id,
                out_window,
                stream,
            }
            .run(),
        );
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
            CommonToWriteMessage::Pull(stream_id, stream, out_window_receiver) => {
                self.process_stream_pull(stream_id, stream, out_window_receiver)?;
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
        Ok(())
    }

    pub fn poll_flush(&mut self, cx: &mut Context<'_>) -> result::Result<()> {
        self.buffer_outg_conn()?;
        loop {
            match self.queued_write.poll(cx) {
                Poll::Pending => return Ok(()),
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Ready(Ok(())) => {}
            }
            let updated = self.buffer_outg_conn()?;
            if !updated {
                return Ok(());
            }
        }
    }
}

// Message sent to write loop.
// Processed while write loop is not handling network I/O.
pub enum CommonToWriteMessage {
    IncreaseInWindow(StreamId, u32),
    StreamEnqueue(StreamId, DataOrHeadersWithFlag),
    StreamEnd(StreamId, ErrorCode), // send when user provided handler completed the stream
    Pull(StreamId, HttpStreamAfterHeaders, StreamOutWindowReceiver),
    DumpState(oneshot::Sender<ConnStateSnapshot>),
}
