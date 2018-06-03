use common::types::Types;
use common::conn::Conn;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use solicit::connection::HttpFrame;
use solicit::StreamId;
use error;
use futures::Poll;
use futures::Async;
use common::conn_write::ConnWriteSideCustom;
use solicit::connection::EndStream;
use Headers;
use result;
use common::stream_map::HttpStreamRef;
use solicit_misc::HttpFrameClassified;
use solicit_misc::HttpFrameStream;
use solicit_misc::HttpFrameConn;
use solicit::frame::PingFrame;
use solicit::frame::GoawayFrame;
use solicit::frame::WindowUpdateFrame;
use solicit::frame::RstStreamFrame;
use solicit::connection::HttpFrameType;
use solicit::frame::SettingsFrame;
use solicit::frame::HttpSetting;
use solicit::MAX_WINDOW_SIZE;
use solicit::frame::PriorityFrame;
use Header;
use ErrorCode;
use solicit::frame::HeadersFrame;
use common::stream::InMessageStage;
use solicit::DEFAULT_SETTINGS;
use solicit::frame::DataFrame;
use solicit::frame::Frame;


pub trait ConnReadSideCustom {
    type Types : Types;

    fn process_headers(&mut self, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<Self::Types>>>;
}

impl<T> Conn<T>
    where
        T : Types,
        Self : ConnReadSideCustom<Types=T>,
        Self : ConnWriteSideCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(&mut self) -> Poll<HttpFrame, error::Error> {
        let max_frame_size = self.our_settings_ack.max_frame_size;

        self.framed_read.poll_http_frame(max_frame_size)
    }

    fn process_data_frame(&mut self, frame: DataFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        let stream_id = frame.get_stream_id();

        self.decrease_in_window(frame.payload_len())?;

        let increment_conn =
        // TODO: need something better
            if self.in_window_size.size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                let increment = DEFAULT_SETTINGS.initial_window_size;
                self.in_window_size.try_increase(increment)
                    .map_err(|()| error::Error::Other("failed to increase window size"))?;

                Some(increment)
            } else {
                None
            };

        let mut error = None;

        loop {
            // If a DATA frame is received whose stream is not in "open" or
            // "half-closed (local)" state, the recipient MUST respond with
            // a stream error (Section 5.4.2) of type STREAM_CLOSED.
            let mut stream = match self.get_stream_maybe_send_error(frame.get_stream_id(), HttpFrameType::Data)? {
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

            assert_eq!(InMessageStage::AfterInitialHeaders, stream.stream().in_message_stage);

            stream.stream().in_window_size.try_decrease_to_positive(frame.payload_len() as i32)
                .map_err(|()| error::Error::CodeError(ErrorCode::FlowControlError))?;

            let end_of_stream = frame.is_end_of_stream();
            stream.stream().new_data_chunk(frame.data, end_of_stream);
            break;
        };

        if let Some(increment_conn) = increment_conn {
            let window_update = WindowUpdateFrame::for_connection(increment_conn);
            self.send_frame_and_notify(window_update);
        }

        if let Some(error) = error {
            self.send_rst_stream(stream_id, error)?;
            return Ok(None);
        }

        Ok(Some(self.streams.get_mut(stream_id).expect("stream must be found")))
    }

    fn process_ping(&mut self, frame: PingFrame) -> result::Result<()> {
        if frame.is_ack() {
            if let Some(opaque_data) = self.ping_sent.take() {
                if opaque_data == frame.opaque_data {
                    Ok(())
                } else {
                    Err(error::Error::Other("PING ACK opaque data mismatch"))
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

    fn process_goaway(&mut self, frame: GoawayFrame) -> result::Result<()> {
        if let Some(..) = self.goaway_received {
            return Err(error::Error::Other("GOAWAY after GOAWAY"));
        }

        let last_stream_id = frame.last_stream_id;
        let raw_error_code = frame.raw_error_code;

        self.goaway_received = Some(frame);

        for (stream_id, mut stream) in self.streams.remove_local_streams_with_id_gt(last_stream_id) {
            debug!("removed stream {} because of GOAWAY", stream_id);
            stream.goaway_recvd(raw_error_code);
        }


        Ok(())
    }

    fn process_headers_frame(&mut self, frame: HeadersFrame) -> result::Result<Option<HttpStreamRef<T>>> {
        let headers = match self.decoder.decode(&frame.header_fragment()) {
            Err(e) => {
                warn!("failed to decode headers: {:?}", e);
                self.send_goaway(ErrorCode::CompressionError)?;
                return Ok(None);
            }
            Ok(headers) => headers,
        };

        let headers = Headers(headers.into_iter().map(|h| Header::new(h.0, h.1)).collect());

        let end_stream = if frame.is_end_of_stream() { EndStream::Yes } else { EndStream::No };

        self.process_headers(frame.stream_id, end_stream, headers)
    }

    fn process_priority_frame(&mut self, frame: PriorityFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        Ok(self.streams.get_mut(frame.get_stream_id()))
    }

    fn process_settings_ack(&mut self, frame: SettingsFrame) -> result::Result<()> {
        assert!(frame.is_ack());

        if let Some(settings) = self.our_settings_sent.take() {
            self.our_settings_ack = settings;
            Ok(())
        } else {
            Err(error::Error::Other("SETTINGS ack without settings sent"))
        }
    }

    fn process_settings_req(&mut self, frame: SettingsFrame) -> result::Result<()> {
        assert!(!frame.is_ack());

        let mut out_window_increased = false;

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
                        for (_, s) in &mut self.streams.map {
                            // In addition to changing the flow-control window for streams
                            // that are not yet active, a SETTINGS frame can alter the initial
                            // flow-control window size for streams with active flow-control windows
                            // (that is, streams in the "open" or "half-closed (remote)" state).
                            // When the value of SETTINGS_INITIAL_WINDOW_SIZE changes,
                            // a receiver MUST adjust the size of all stream flow-control windows
                            // that it maintains by the difference between the new value
                            // and the old value.
                            // TODO: check for overflow
                            s.out_window_size.0 += delta;
                            s.pump_out_window.increase(delta);
                        }

                        if !self.streams.map.is_empty() && delta > 0 {
                            out_window_increased = true;
                        }
                    }
                }
                HttpSetting::HeaderTableSize(_new_size) => {
                }
                _ => {}
            }

            self.peer_settings.apply(setting);
        }

        self.send_ack_settings()?;

        if out_window_increased {
            self.out_window_increased(None)?;
        }

        Ok(())
    }

    fn process_settings(&mut self, frame: SettingsFrame) -> result::Result<()> {
        if frame.is_ack() {
            self.process_settings_ack(frame)
        } else {
            self.process_settings_req(frame)
        }
    }

    fn process_stream_window_update_frame(&mut self, frame: WindowUpdateFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        self.out_window_increased(Some(frame.stream_id))?;



        match self.get_stream_maybe_send_error(frame.stream_id, HttpFrameType::WindowUpdate)? {
            Some(..) => {}
            None => {
                // 6.9
                // WINDOW_UPDATE can be sent by a peer that has sent a frame bearing the
                // END_STREAM flag.  This means that a receiver could receive a
                // WINDOW_UPDATE frame on a "half-closed (remote)" or "closed" stream.
                // A receiver MUST NOT treat this as an error (see Section 5.1).
                debug!("WINDOW_UPDATE of unknown stream: {}", frame.get_stream_id());
                return Ok(None);
            }
        }

        // Work arout lexical lifetimes

        let old_window_size = self.streams.get_mut(frame.stream_id).unwrap()
            .stream().out_window_size.0;

        // 6.9.1
        // A sender MUST NOT allow a flow-control window to exceed 2^31-1
        // octets.  If a sender receives a WINDOW_UPDATE that causes a flow-
        // control window to exceed this maximum, it MUST terminate either the
        // stream or the connection, as appropriate.  For streams, the sender
        // sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the
        // connection, a GOAWAY frame with an error code of FLOW_CONTROL_ERROR
        // is sent.
        if let Err(..) = self.streams.get_mut(frame.stream_id).unwrap()
            .stream().out_window_size.try_increase(frame.increment)
            {
                info!("failed to increment stream window: {}", frame.stream_id);
                self.send_rst_stream(frame.stream_id, ErrorCode::FlowControlError)?;
                return Ok(None);
            }

        let mut stream = self.streams.get_mut(frame.stream_id).unwrap();

        let new_window_size = stream.stream().out_window_size.0;

        debug!("stream {} out window size change: {} -> {}",
            frame.stream_id, old_window_size, new_window_size);

        stream.stream().pump_out_window.increase(frame.increment as i32);

        Ok(Some(stream))
    }

    fn process_conn_window_update(&mut self, frame: WindowUpdateFrame) -> result::Result<()> {
        assert_eq!(0, frame.stream_id);

        let old_window_size = self.out_window_size.0;

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

        debug!("conn out window size change: {} -> {}", old_window_size, self.out_window_size);

        self.pump_out_window_size.increase(frame.increment);

        self.out_window_increased(None)
    }

    fn process_rst_stream_frame(&mut self, frame: RstStreamFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        let stream_id = frame.get_stream_id();
        if let Some(stream) = self.get_stream_maybe_send_error(stream_id, HttpFrameType::RstStream)? {
            stream.rst_received_remove(frame.error_code());
        }

        self.peer_closed_streams.add(stream_id);

        Ok(None)
    }

    fn process_conn_frame(&mut self, frame: HttpFrameConn) -> result::Result<()> {
        match frame {
            HttpFrameConn::Settings(f) => self.process_settings(f),
            HttpFrameConn::Ping(f) => self.process_ping(f),
            HttpFrameConn::Goaway(f) => self.process_goaway(f),
            HttpFrameConn::WindowUpdate(f) => self.process_conn_window_update(f),
        }
    }

    fn process_stream_frame(&mut self, frame: HttpFrameStream) -> result::Result<()> {
        let stream_id = frame.get_stream_id();
        let end_of_stream = frame.is_end_of_stream();

        // 6.8
        // Once sent, the sender will ignore frames sent on streams initiated by the receiver
        // if the stream has an identifier higher than the included last stream identifier.
        if let Some(ref f) = self.goaway_sent.as_ref() {
            if !T::is_init_locally(stream_id) {
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
                HttpFrameStream::PushPromise(_f) => return Err(error::Error::NotImplemented("PUSH_PROMISE")),
                HttpFrameStream::WindowUpdate(window_update) => self.process_stream_window_update_frame(window_update)?,
                HttpFrameStream::Continuation(_continuation) => unreachable!("must be joined with HEADERS before that"),
            };

            if let Some(mut stream) = stream {
                if end_of_stream {
                    stream.stream().close_remote();
                }
                stream.remove_if_closed();
            }
        }

        if end_of_stream {
            self.peer_closed_streams.add(stream_id);
        }

        Ok(())
    }

    fn process_http_frame(&mut self, frame: HttpFrame) -> result::Result<()> {
        // TODO: decode headers
        debug!("received frame: {:?}", frame);
        match HttpFrameClassified::from(frame) {
            HttpFrameClassified::Conn(f) => self.process_conn_frame(f),
            HttpFrameClassified::Stream(f) => self.process_stream_frame(f),
            HttpFrameClassified::Unknown(_f) => {
                // 4.1
                // Implementations MUST ignore and discard any frame that has a type that is unknown.
                Ok(())
            },
        }
    }

    /// Loop forever, never return `Ready`
    pub fn read_process_frame(&mut self) -> Poll<(), error::Error> {
        loop {
            if self.end_loop() {
                return Err(error::Error::Other("GOAWAY"));
            }

            let frame = match self.recv_http_frame()? {
                Async::Ready(frame) => frame,
                Async::NotReady => return Ok(Async::NotReady),
            };

            self.process_http_frame(frame)?;
        }
    }
}
