use crate::codec::http_decode_read::HttpFrameDecodedOrGoaway;
use crate::common::conn::Conn;
use crate::common::conn_write::ConnWriteSideCustom;
use crate::common::init_where::InitWhere;
use crate::common::stream::DroppedData;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::stream::InMessageStage;
use crate::common::stream_map::HttpStreamRef;
use crate::common::types::Types;
use crate::error;
use crate::solicit::end_stream::EndStream;
use crate::solicit::frame::DataFrame;
use crate::solicit::frame::Frame;
use crate::solicit::frame::GoawayFrame;
use crate::solicit::frame::HeadersDecodedFrame;
use crate::solicit::frame::HttpFrameDecoded;
use crate::solicit::frame::HttpFrameType;
use crate::solicit::frame::HttpSetting;
use crate::solicit::frame::PingFrame;
use crate::solicit::frame::PriorityFrame;
use crate::solicit::frame::RstStreamFrame;
use crate::solicit::frame::SettingsFrame;
use crate::solicit::frame::WindowUpdateFrame;
use crate::solicit::stream_id::StreamId;
use crate::solicit::window_size::MAX_WINDOW_SIZE;
use crate::solicit::DEFAULT_SETTINGS;
use crate::solicit_misc::HttpFrameClassified;
use crate::solicit_misc::HttpFrameConn;
use crate::solicit_misc::HttpFrameStream;
use crate::ErrorCode;
use crate::Headers;

use futures::task::Context;
use std::task::Poll;
use tls_api::AsyncSocket;

pub(crate) trait ConnReadSideCustom {
    type Types: Types;

    fn process_headers(
        &mut self,
        stream_id: StreamId,
        end_stream: EndStream,
        headers: Headers,
    ) -> crate::Result<Option<HttpStreamRef<Self::Types>>>;
}

impl<T, I> Conn<T, I>
where
    T: Types,
    Self: ConnReadSideCustom<Types = T>,
    Self: ConnWriteSideCustom<Types = T>,
    HttpStreamCommon<T>: HttpStreamData<Types = T>,
    I: AsyncSocket,
{
    /// Recv a frame from the network
    pub fn poll_recv_http_frame(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<crate::Result<HttpFrameDecodedOrGoaway>> {
        let max_frame_size = self.our_settings_ack.max_frame_size;

        self.framed_read.poll_http_frame(cx, max_frame_size)
    }

    fn process_data_frame(&mut self, frame: DataFrame) -> crate::Result<Option<HttpStreamRef<T>>> {
        let stream_id = frame.get_stream_id();

        self.decrease_in_window(frame.payload_len())?;

        let increment_conn =
        // TODO: need something better
            if self.in_window_size.size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                let increment = DEFAULT_SETTINGS.initial_window_size;
                let old_in_window_size = self.in_window_size.size();
                self.in_window_size.try_increase(increment)
                    .map_err(|()| error::Error::ConnInWindowOverflow(self.in_window_size.size(), increment))?;
                let new_in_window_size = self.in_window_size.size();
                debug!("requesting increase in window: {} -> {}", old_in_window_size, new_in_window_size);

                Some(increment)
            } else {
                None
            };

        let mut error = None;

        loop {
            // If a DATA frame is received whose stream is not in "open" or
            // "half-closed (local)" state, the recipient MUST respond with
            // a stream error (Section 5.4.2) of type STREAM_CLOSED.
            let mut stream = match self
                .get_stream_maybe_send_error(frame.get_stream_id(), HttpFrameType::Data)?
            {
                Some(stream) => stream,
                None => {
                    return Ok(None);
                }
            };

            if let Some(in_rem_content_length) = stream.stream().in_rem_content_length {
                if in_rem_content_length < frame.data.len() as u64 {
                    warn!("stream data underflow content-length");
                    error = Some(ErrorCode::ProtocolError);
                    break;
                }

                let in_rem_content_length = in_rem_content_length - frame.data.len() as u64;
                stream.stream().in_rem_content_length = Some(in_rem_content_length);
            }

            assert_eq!(
                InMessageStage::AfterInitialHeaders,
                stream.stream().in_message_stage
            );

            let old_in_window_size = stream.stream().in_window_size.size();
            stream
                .stream()
                .in_window_size
                .try_decrease_to_non_negative(frame.payload_len() as i32)
                .map_err(|()| error::Error::CodeError(ErrorCode::FlowControlError))?;
            let new_in_window_size = stream.stream().in_window_size.size();

            debug!(
                "decrease stream {} window: {} -> {}",
                stream.id(),
                old_in_window_size,
                new_in_window_size
            );

            let end_of_stream = frame.is_end_of_stream();
            stream
                .stream()
                .data_recvd(frame.data, EndStream::from_bool(end_of_stream));
            break;
        }

        if let Some(increment_conn) = increment_conn {
            let window_update = WindowUpdateFrame::for_connection(increment_conn);
            self.send_frame_and_notify(window_update);
        }

        if let Some(error) = error {
            self.send_rst_stream(stream_id, error)?;
            return Ok(None);
        }

        Ok(Some(
            self.streams
                .get_mut(stream_id)
                .expect("stream must be found"),
        ))
    }

    fn process_ping(&mut self, frame: PingFrame) -> crate::Result<()> {
        if frame.is_ack() {
            if let Some(opaque_data) = self.ping_sent.take() {
                if opaque_data == frame.opaque_data {
                    Ok(())
                } else {
                    Err(error::Error::PingAckOpaqueDataMismatch(
                        opaque_data,
                        frame.opaque_data,
                    ))
                }
            } else {
                warn!("PING ACK without PING");
                Ok(())
            }
        } else {
            let ping = PingFrame::new_ack(frame.opaque_data());
            self.send_frame_and_notify(ping);
            Ok(())
        }
    }

    fn process_goaway(&mut self, frame: GoawayFrame) -> crate::Result<()> {
        if let Some(..) = self.goaway_received {
            return Err(error::Error::GoawayAfterGoaway);
        }

        let last_stream_id = frame.last_stream_id;
        let raw_error_code = frame.error_code.0;

        self.goaway_received = Some(frame);

        for (stream_id, mut stream) in self.streams.remove_local_streams_with_id_gt(last_stream_id)
        {
            debug!("removed stream {} because of GOAWAY", stream_id);
            stream.goaway_recvd(raw_error_code);
        }

        Ok(())
    }

    fn process_headers_frame(
        &mut self,
        frame: HeadersDecodedFrame,
    ) -> crate::Result<Option<HttpStreamRef<T>>> {
        let end_stream = if frame.is_end_of_stream() {
            EndStream::Yes
        } else {
            EndStream::No
        };

        self.process_headers(frame.stream_id, end_stream, frame.headers)
    }

    fn process_priority_frame(
        &mut self,
        frame: PriorityFrame,
    ) -> crate::Result<Option<HttpStreamRef<T>>> {
        Ok(self.streams.get_mut(frame.get_stream_id()))
    }

    fn process_settings_ack(&mut self, frame: SettingsFrame) -> crate::Result<()> {
        assert!(frame.is_ack());

        self.our_settings_ack = self.our_settings_sent;
        Ok(())
    }

    fn process_settings_req(&mut self, frame: SettingsFrame) -> crate::Result<()> {
        assert!(!frame.is_ack());

        for setting in frame.settings {
            match setting {
                HttpSetting::InitialWindowSize(new_size) => {
                    // 6.5.2
                    // Values above the maximum flow-control window size of 2^31-1 MUST
                    // be treated as a connection error (Section 5.4.1) of type
                    // FLOW_CONTROL_ERROR.
                    if new_size > MAX_WINDOW_SIZE {
                        self.send_flow_control_error()?;
                        return Ok(());
                    }

                    let old_size = self.peer_settings.initial_window_size;
                    let delta = (new_size as i32) - (old_size as i32);

                    if delta != 0 {
                        self.streams.add_out_window(delta);
                    }
                }
                HttpSetting::HeaderTableSize(_new_size) => {}
                _ => {}
            }

            self.peer_settings.apply(setting);
        }

        self.send_ack_settings()?;

        Ok(())
    }

    fn process_settings(&mut self, frame: SettingsFrame) -> crate::Result<()> {
        if frame.is_ack() {
            self.process_settings_ack(frame)
        } else {
            self.process_settings_req(frame)
        }
    }

    fn process_stream_window_update_frame(
        &mut self,
        frame: WindowUpdateFrame,
    ) -> crate::Result<Option<HttpStreamRef<T>>> {
        let mut stream =
            match self.get_stream_maybe_send_error(frame.stream_id, HttpFrameType::WindowUpdate)? {
                Some(s) => s,
                None => {
                    // 6.9
                    // WINDOW_UPDATE can be sent by a peer that has sent a frame bearing the
                    // END_STREAM flag.  This means that a receiver could receive a
                    // WINDOW_UPDATE frame on a "half-closed (remote)" or "closed" stream.
                    // A receiver MUST NOT treat this as an error (see Section 5.1).
                    debug!("WINDOW_UPDATE of unknown stream: {}", frame.get_stream_id());
                    return Ok(None);
                }
            };

        // 6.9.1
        // A sender MUST NOT allow a flow-control window to exceed 2^31-1
        // octets.  If a sender receives a WINDOW_UPDATE that causes a flow-
        // control window to exceed this maximum, it MUST terminate either the
        // stream or the connection, as appropriate.  For streams, the sender
        // sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the
        // connection, a GOAWAY frame with an error code of FLOW_CONTROL_ERROR
        // is sent.
        if let Err(..) = stream.try_increase_window_size(frame.increment) {
            info!("failed to increment stream window: {}", frame.stream_id);
            self.send_rst_stream(frame.stream_id, ErrorCode::FlowControlError)?;
            return Ok(None);
        }

        let mut stream = self.streams.get_mut(frame.stream_id).unwrap();

        // TODO: push pump increase inside stream.try_increase_window_size
        stream
            .stream()
            .pump_out_window
            .increase(frame.increment as isize);

        Ok(Some(stream))
    }

    fn process_conn_window_update(&mut self, frame: WindowUpdateFrame) -> crate::Result<()> {
        assert_eq!(0, frame.stream_id);

        let old_window_size = self.out_window_size.size();

        // 6.9.1
        // A sender MUST NOT allow a flow-control window to exceed 2^31-1
        // octets.  If a sender receives a WINDOW_UPDATE that causes a flow-
        // control window to exceed this maximum, it MUST terminate either the
        // stream or the connection, as appropriate.  For streams, the sender
        // sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the
        // connection, a GOAWAY frame with an error code of FLOW_CONTROL_ERROR
        // is sent.
        if let Err(_) = self.out_window_size.try_increase(frame.increment) {
            info!("attempted to increase window size too far");
            self.send_flow_control_error()?;
            return Ok(());
        }

        debug!(
            "conn out window size change: {} -> {}",
            old_window_size, self.out_window_size
        );

        self.pump_out_window_size.increase(frame.increment as usize);
        Ok(())
    }

    fn process_rst_stream_frame(
        &mut self,
        frame: RstStreamFrame,
    ) -> crate::Result<Option<HttpStreamRef<T>>> {
        let stream_id = frame.get_stream_id();
        let dropped_data = if let Some(stream) =
            self.get_stream_maybe_send_error(stream_id, HttpFrameType::RstStream)?
        {
            stream.rst_received_remove(frame.error_code())
        } else {
            DroppedData { size: 0 }
        };

        {
            let DroppedData { size } = dropped_data;
            self.pump_out_window_size.increase(size);
        }

        self.peer_closed_streams.add(stream_id);

        Ok(None)
    }

    fn process_conn_frame(&mut self, frame: HttpFrameConn) -> crate::Result<()> {
        match frame {
            HttpFrameConn::Settings(f) => self.process_settings(f),
            HttpFrameConn::Ping(f) => self.process_ping(f),
            HttpFrameConn::Goaway(f) => self.process_goaway(f),
            HttpFrameConn::WindowUpdate(f) => self.process_conn_window_update(f),
        }
    }

    fn process_stream_frame(&mut self, frame: HttpFrameStream) -> crate::Result<()> {
        let stream_id = frame.get_stream_id();
        let end_of_stream = frame.is_end_of_stream();

        // 6.8
        // Once sent, the sender will ignore frames sent on streams initiated by the receiver
        // if the stream has an identifier higher than the included last stream identifier.
        if let Some(ref f) = self.goaway_sent.as_ref() {
            if T::init_where(stream_id) != InitWhere::Locally {
                if stream_id > f.last_stream_id {
                    return Ok(());
                }
            }
        }

        {
            let stream = match frame {
                HttpFrameStream::Data(data) => self.process_data_frame(data)?,
                HttpFrameStream::Headers(headers) => self.process_headers_frame(headers)?,
                HttpFrameStream::Priority(priority) => self.process_priority_frame(priority)?,
                HttpFrameStream::RstStream(rst) => self.process_rst_stream_frame(rst)?,
                HttpFrameStream::PushPromise(_f) => {
                    return Err(error::Error::UnexpectedPushPromise)
                }
                HttpFrameStream::WindowUpdate(window_update) => {
                    self.process_stream_window_update_frame(window_update)?
                }
            };

            if let Some(stream) = stream {
                if end_of_stream {
                    stream.close_remote();
                }
            }
        }

        if end_of_stream {
            self.peer_closed_streams.add(stream_id);
        }

        Ok(())
    }

    fn process_http_frame(&mut self, frame: HttpFrameDecoded) -> crate::Result<()> {
        if log_enabled!(log::Level::Trace) {
            debug!("received frame: {:?}", frame);
        } else {
            debug!("received frame: {:?}", frame.debug_no_data());
        }
        match HttpFrameClassified::from(frame) {
            HttpFrameClassified::Conn(f) => self.process_conn_frame(f),
            HttpFrameClassified::Stream(f) => self.process_stream_frame(f),
            HttpFrameClassified::Unknown(_f) => {
                // 4.1
                // Implementations MUST ignore and discard any frame that has a type that is unknown.
                Ok(())
            }
        }
    }

    /// Send `RST_STREAM` when received incorrect stream frame
    fn process_stream_error(
        &mut self,
        stream_id: StreamId,
        error_code: ErrorCode,
    ) -> crate::Result<()> {
        if let Some(mut stream) = self.streams.get_mut(stream_id) {
            stream.close_outgoing(error_code);
        } else {
            self.queued_write
                .queue_not_goaway(RstStreamFrame::new(stream_id, error_code));
        }
        Ok(())
    }

    pub fn process_http_frame_of_goaway(
        &mut self,
        m: HttpFrameDecodedOrGoaway,
    ) -> crate::Result<()> {
        match m {
            HttpFrameDecodedOrGoaway::Frame(frame) => self.process_http_frame(frame),
            HttpFrameDecodedOrGoaway::_SendRst(stream_id, error_code) => {
                self.process_stream_error(stream_id, error_code)
            }
            HttpFrameDecodedOrGoaway::SendGoaway(error_code) => self.send_goaway(error_code),
        }
    }
}
