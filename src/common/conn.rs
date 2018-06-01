use std::collections::HashMap;
use std::cmp;

use futures::sync::mpsc::UnboundedSender;

use tokio_core::reactor;

use exec::Executor;
use exec::CpuPoolOption;

use error;
use error::ErrorCode;
use result;

use solicit::frame::*;
use solicit::header::*;
use solicit::frame::continuation::*;
use solicit::StreamId;
use solicit::DEFAULT_SETTINGS;
use solicit::connection::EndStream;
use solicit::connection::HttpConnection;
use solicit::connection::HttpFrame;
use solicit::connection::HttpFrameType;
use solicit::frame::settings::HttpSettings;
use solicit::session::StreamState;
use solicit::session::StreamStateIdleOrClosed;

use solicit_misc::*;

use super::stream::*;
use super::stream_map::*;
use super::closed_streams::*;
use super::types::*;
use super::conf::*;
use super::pump_stream_to_write_loop::PumpStreamToWriteLoop;
use super::stream_from_network::StreamFromNetwork;
use super::stream_queue_sync::StreamQueueSyncReceiver;
use super::window_size;
use super::stream_queue_sync::stream_queue_sync;
use super::conn_write_loop::DirectlyToNetworkFrame;


pub use resp::Response;

use rc_mut::*;
use solicit::MAX_WINDOW_SIZE;
use client_died_error_holder::ClientDiedErrorHolder;
use client_died_error_holder::ClientConnDiedType;
use data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use common::conn_write_loop::CommonToWriteMessage;
use common::conn_write_loop::WriteLoop;
use common::conn_read_loop::ReadLoop;
use common::conn_command_loop::CommandLoop;
use tokio_io::AsyncWrite;
use tokio_io::AsyncRead;
use codec::http_framed_read::HttpFramedJoinContinuationRead;
use codec::http_framed_write::HttpFramedWrite;
use solicit_async::HttpFutureStreamSend;
use tokio_io::io::WriteHalf;
use tokio_io::io::ReadHalf;
use common::conn_write_loop::WriteLoopCustom;
use common::conn_read_loop::ReadLoopCustom;
use common::conn_command_loop::CommandLoopCustom;
use futures::future;
use futures::Future;
use futures::Poll;
use futures::Async;


pub trait ConnDataSpecific : 'static {
}


pub struct ConnData<T : Types> {
    pub conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,

    /// Client or server specific data
    pub specific: T::ConnDataSpecific,
    /// Messages to be sent to write loop
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
    /// Reactor we are using
    pub loop_handle: reactor::Handle,
    /// Executor which drives requests on client and responses on server
    pub exec: Box<Executor>,
    /// Connection state
    pub conn: HttpConnection,
    /// Known streams
    pub streams: StreamMap<T>,
    /// Last streams known to be closed by peer
    pub peer_closed_streams: ClosedStreams,

    /// Window size from pumper point of view
    pub pump_out_window_size: window_size::ConnOutWindowSender,

    pub last_local_stream_id: StreamId,
    pub last_peer_stream_id: StreamId,
    pub goaway_sent: Option<GoawayFrame>,
    pub goaway_received: Option<GoawayFrame>,
    pub ping_sent: Option<u64>,
}


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ConnectionStateSnapshot {
    pub in_window_size: i32,
    pub out_window_size: i32,
    pub streams: HashMap<StreamId, HttpStreamStateSnapshot>,
}

impl ConnectionStateSnapshot {
    pub fn single_stream(&self) -> (u32, &HttpStreamStateSnapshot) {
        let mut iter = self.streams.iter();
        let (&id, stream) = iter.next().expect("no streams");
        assert!(iter.next().is_none(), "more than one stream");
        (id, stream)
    }
}



impl<T : Types> ConnData<T>
    where
        Self : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub fn new(
        loop_handle: reactor::Handle,
        exec: CpuPoolOption,
        specific: T::ConnDataSpecific,
        _conf: CommonConf,
        sent_settings: HttpSettings,
        to_write_tx: UnboundedSender<T::ToWriteMessage>)
            -> ConnData<T>
    {
        let mut conn = HttpConnection::new();
        conn.our_settings_sent = Some(sent_settings);

        let pump_window_size = window_size::ConnOutWindowSender::new(conn.out_window_size.0 as u32);

        ConnData {
            conn_died_error_holder: ClientDiedErrorHolder::new(),
            specific: specific,
            to_write_tx: to_write_tx,
            conn: conn,
            streams: StreamMap::new(),
            last_local_stream_id: 0,
            last_peer_stream_id: 0,
            exec: exec.make_executor(&loop_handle),
            loop_handle: loop_handle,
            goaway_sent: None,
            goaway_received: None,
            ping_sent: None,
            pump_out_window_size: pump_window_size,
            peer_closed_streams: ClosedStreams::new(),
        }
    }

    /// Allocate stream id for locally initiated stream
    pub fn next_local_stream_id(&mut self) -> StreamId {
        let id = match self.last_local_stream_id {
            0 => T::first_id(),
            n => n + 2,
        };
        self.last_local_stream_id = id;
        id
    }


    pub fn new_stream_data(
        &mut self,
        stream_id: StreamId,
        in_rem_content_length: Option<u64>,
        in_message_stage: InMessageStage,
        specific: T::HttpStreamSpecific)
        -> (HttpStreamRef<T>, StreamFromNetwork<T>, window_size::StreamOutWindowReceiver)
    {
        let (inc_tx, inc_rx) = stream_queue_sync(self.conn_died_error_holder.clone());

        let in_window_size = self.conn.our_settings_sent().initial_window_size;

        let stream_from_network = self.new_stream_from_network(
            inc_rx,
            stream_id,
            in_window_size);

        let (out_window_sender, out_window_receiver) =
            self.pump_out_window_size.new_stream(self.conn.peer_settings.initial_window_size as u32);

        let stream = HttpStreamCommon::new(
            in_window_size,
            self.conn.peer_settings.initial_window_size,
            inc_tx,
            out_window_sender,
            in_rem_content_length,
            in_message_stage,
            specific);

        let stream = self.streams.insert(stream_id, stream);

        (stream, stream_from_network, out_window_receiver)
    }

    fn new_stream_from_network(
        &self,
        rx: StreamQueueSyncReceiver,
        stream_id: StreamId,
        in_window_size: u32)
            -> StreamFromNetwork<T>
    {
        StreamFromNetwork {
            rx: rx,
            stream_id: stream_id,
            to_write_tx: self.to_write_tx.clone(),
            in_window_size: in_window_size,
        }
    }


    pub fn pop_outg_all_for_stream(&mut self, stream_id: StreamId) -> Vec<HttpStreamCommand> {
        if let Some(stream) = self.streams.get_mut(stream_id) {
            stream.pop_outg_all_maybe_remove(&mut self.conn.out_window_size)
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

    fn write_part(&mut self, target: &mut FrameBuilder, stream_id: StreamId, part: HttpStreamCommand) {
        let max_frame_size = self.conn.peer_settings.max_frame_size as usize;

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
                    .conn.encoder.encode(headers.0.iter().map(|h| (h.name(), h.value())));

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

    pub fn pop_outg_all_for_stream_bytes(&mut self, stream_id: StreamId) -> Vec<u8> {
        let mut send = FrameBuilder::new();
        for part in self.pop_outg_all_for_stream(stream_id) {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    pub fn pop_outg_all_for_conn_bytes(&mut self) -> Vec<u8> {
        // TODO: maintain own limits of out window
        let mut send = FrameBuilder::new();
        for (stream_id, part) in self.pop_outg_all_for_conn() {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    pub fn dump_state(&self) -> ConnectionStateSnapshot {
        ConnectionStateSnapshot {
            in_window_size: self.conn.in_window_size.0,
            out_window_size: self.conn.out_window_size.0,
            streams: self.streams.snapshot(),
        }
    }

    fn process_headers_frame(&mut self, self_rc: RcMut<Self>, frame: HeadersFrame) -> result::Result<Option<HttpStreamRef<T>>> {
        let headers = match self.conn.decoder.decode(&frame.header_fragment()) {
            Err(e) => {
                warn!("failed to decode headers: {:?}", e);
                self.send_goaway(ErrorCode::CompressionError)?;
                return Ok(None);
            }
            Ok(headers) => headers,
        };

        let headers = Headers(headers.into_iter().map(|h| Header::new(h.0, h.1)).collect());

        let end_stream = if frame.is_end_of_stream() { EndStream::Yes } else { EndStream::No };

        self.process_headers(self_rc, frame.stream_id, end_stream, headers)
    }

    fn process_priority_frame(&mut self, frame: PriorityFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        Ok(self.streams.get_mut(frame.get_stream_id()))
    }

    fn process_settings_ack(&mut self, frame: SettingsFrame) -> result::Result<()> {
        assert!(frame.is_ack());

        if let Some(settings) = self.conn.our_settings_sent.take() {
            self.conn.our_settings_ack = settings;
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

                    let old_size = self.conn.peer_settings.initial_window_size;
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

            self.conn.peer_settings.apply(setting);
        }

        self.ack_settings()?;

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

        let old_window_size = self.conn.out_window_size.0;

        // 6.9.1
        // A sender MUST NOT allow a flow-control window to exceed 2^31-1
        // octets.  If a sender receives a WINDOW_UPDATE that causes a flow-
        // control window to exceed this maximum, it MUST terminate either the
        // stream or the connection, as appropriate.  For streams, the sender
        // sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the
        // connection, a GOAWAY frame with an error code of FLOW_CONTROL_ERROR
        // is sent.
        if let Err(_) = self.conn.out_window_size.try_increase(frame.increment) {
            info!("attempted to increase window size too far");
            self.send_flow_control_error()?;
            return Ok(());
        }

        debug!("conn out window size change: {} -> {}", old_window_size, self.conn.out_window_size);

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

    pub fn send_rst_stream(&mut self, stream_id: StreamId, error_code: ErrorCode)
        -> result::Result<()>
    {
        // TODO: probably notify handlers
        self.streams.remove_stream(stream_id);

        let rst_stream = RstStreamFrame::new(stream_id, error_code);
        self.send_directly_to_network(DirectlyToNetworkFrame::RstStream(rst_stream))
    }

    fn send_goaway(&mut self, error_code: ErrorCode)
        -> result::Result<()>
    {
        let goaway = GoawayFrame::new(self.last_peer_stream_id, error_code);
        self.send_directly_to_network(DirectlyToNetworkFrame::GoAway(goaway))?;
        self.send_common(CommonToWriteMessage::CloseConn)?;
        Ok(())
    }

    fn send_flow_control_error(&mut self) -> result::Result<()> {
        self.send_goaway(ErrorCode::FlowControlError)
    }

    fn stream_state_idle_or_closed(&self, stream_id: StreamId) -> StreamStateIdleOrClosed {
        let last_stream_id =
            if T::is_init_locally(stream_id) {
                self.last_local_stream_id
            } else {
                self.last_peer_stream_id
            };

        if stream_id > last_stream_id {
            StreamStateIdleOrClosed::Idle
        } else {
            StreamStateIdleOrClosed::Closed
        }
    }

    fn stream_state(&self, stream_id: StreamId) -> StreamState {
        match self.streams.get_stream_state(stream_id) {
            Some(state) => state,
            None => self.stream_state_idle_or_closed(stream_id).into(),
        }
    }

    pub fn get_stream_maybe_send_error(&mut self, stream_id: StreamId, frame_type: HttpFrameType)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        let stream_state = self.stream_state(stream_id);

        match stream_state {
            StreamState::Idle => {
                let send_connection_error = match frame_type {
                    HttpFrameType::Headers |
                    HttpFrameType::Priority |
                    HttpFrameType::PushPromise => false,
                    _ => true,
                };

                if send_connection_error {
                    debug!("stream is idle: {}, sending GOAWAY", stream_id);
                    self.send_goaway(ErrorCode::StreamClosed)?;
                }
            }
            StreamState::Open | StreamState::HalfClosedLocal => {}
            // TODO
            StreamState::ReservedLocal | StreamState::ReservedRemote => {}
            StreamState::HalfClosedRemote => {
                // If an endpoint receives additional frames, other than
                // WINDOW_UPDATE, PRIORITY, or RST_STREAM, for a stream that is in
                // this state, it MUST respond with a stream error (Section 5.4.2) of
                // type STREAM_CLOSED.
                let send_rst = match frame_type {
                    HttpFrameType::WindowUpdate |
                    HttpFrameType::Priority |
                    HttpFrameType::RstStream => false,
                    _ => true,
                };
                
                if send_rst {
                    debug!("stream is half-closed remote: {}, sending RST_STREAM", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::StreamClosed)?;
                }
            }
            StreamState::Closed => {
                // An endpoint MUST NOT send frames other than PRIORITY on a closed
                // stream.  An endpoint that receives any frame other than PRIORITY
                // after receiving a RST_STREAM MUST treat that as a stream error
                // (Section 5.4.2) of type STREAM_CLOSED.  Similarly, an endpoint
                // that receives any frames after receiving a frame with the
                // END_STREAM flag set MUST treat that as a connection error
                // (Section 5.4.1) of type STREAM_CLOSED, unless the frame is
                // permitted as described below.
                //
                // WINDOW_UPDATE or RST_STREAM frames can be received in this state
                // for a short period after a DATA or HEADERS frame containing an
                // END_STREAM flag is sent.  Until the remote peer receives and
                // processes RST_STREAM or the frame bearing the END_STREAM flag, it
                // might send frames of these types.  Endpoints MUST ignore
                // WINDOW_UPDATE or RST_STREAM frames received in this state, though
                // endpoints MAY choose to treat frames that arrive a significant
                // time after sending END_STREAM as a connection error
                // (Section 5.4.1) of type PROTOCOL_ERROR.

                let send_stream_closed = match frame_type {
                    HttpFrameType::RstStream |
                    HttpFrameType::Priority |
                    HttpFrameType::WindowUpdate => false,
                    _ => true,
                };

                // TODO: http2 spec requires sending stream or connection error
                // depending on how stream was closed
                if send_stream_closed {

                    if self.peer_closed_streams.contains(stream_id) {
                        debug!("stream is closed by peer: {}, sending GOAWAY", stream_id);
                        self.send_goaway(ErrorCode::StreamClosed)?;
                    } else {
                        debug!("stream is closed by us: {}, sending RST_STREAM", stream_id);
                        self.send_rst_stream(stream_id, ErrorCode::StreamClosed)?;
                    }
                }
            }
        }

        Ok(self.streams.get_mut(stream_id))
    }

    pub fn get_stream_for_headers_maybe_send_error(&mut self, stream_id: StreamId)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        self.get_stream_maybe_send_error(stream_id, HttpFrameType::Headers)
    }

    fn process_data_frame(&mut self, frame: DataFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        let stream_id = frame.get_stream_id();

        self.conn.decrease_in_window(frame.payload_len())?;

        let increment_conn =
            // TODO: need something better
            if self.conn.in_window_size.size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                let increment = DEFAULT_SETTINGS.initial_window_size;
                self.conn.in_window_size.try_increase(increment)
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
            self.send_directly_to_network(DirectlyToNetworkFrame::WindowUpdate(window_update))?;
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
            self.send_directly_to_network(DirectlyToNetworkFrame::Ping(ping))
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

    fn process_conn_frame(&mut self, frame: HttpFrameConn) -> result::Result<()> {
        match frame {
            HttpFrameConn::Settings(f) => self.process_settings(f),
            HttpFrameConn::Ping(f) => self.process_ping(f),
            HttpFrameConn::Goaway(f) => self.process_goaway(f),
            HttpFrameConn::WindowUpdate(f) => self.process_conn_window_update(f),
        }
    }

    fn process_stream_frame(&mut self, self_rc: RcMut<Self>, frame: HttpFrameStream) -> result::Result<()> {
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
                HttpFrameStream::Headers(headers) => self.process_headers_frame(self_rc, headers)?,
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

    pub fn process_http_frame(&mut self, self_rc: RcMut<Self>, frame: HttpFrame) -> result::Result<()> {
        // TODO: decode headers
        debug!("received frame: {:?}", frame);
        match HttpFrameClassified::from(frame) {
            HttpFrameClassified::Conn(f) => self.process_conn_frame(f),
            HttpFrameClassified::Stream(f) => self.process_stream_frame(self_rc, f),
            HttpFrameClassified::Unknown(_f) => {
                // 4.1
                // Implementations MUST ignore and discard any frame that has a type that is unknown.
                Ok(())
            },
        }
    }

    fn send_common(&mut self, message: CommonToWriteMessage)
            -> result::Result<()>
        where T::ToWriteMessage : From<CommonToWriteMessage>
    {
        self.to_write_tx.unbounded_send(message.into())
            .map_err(|_| error::Error::Other("write loop died"))
    }

    /// Schedule a write for HTTP frame
    /// Must not be data frame
    fn send_directly_to_network(&mut self, frame: DirectlyToNetworkFrame) -> result::Result<()> {
        self.send_common(CommonToWriteMessage::Frame(frame))
    }

    fn out_window_increased(&mut self, stream_id: Option<StreamId>) -> result::Result<()> {
        self.send_common(CommonToWriteMessage::TryFlushStream(stream_id))
    }

    /// Sends an SETTINGS Frame with ack set to acknowledge seeing a SETTINGS frame from the peer.
    fn ack_settings(&mut self) -> result::Result<()> {
        let settings = SettingsFrame::new_ack();
        self.send_directly_to_network(DirectlyToNetworkFrame::Settings(settings))
    }

    /// Should we close the connection because of GOAWAY state
    pub fn end_loop(&self) -> bool {
        let goaway = self.goaway_sent.is_some() || self.goaway_received.is_some();
        let no_streams = self.streams.is_empty();
        goaway && no_streams
    }

    pub fn new_pump_stream_to_write_loop(
        &self,
        stream_id: StreamId,
        stream: DataOrHeadersWithFlagStream,
        out_window: window_size::StreamOutWindowReceiver)
        -> PumpStreamToWriteLoop<T>
    {
        let stream = stream.catch_unwind();
        PumpStreamToWriteLoop {
            to_write_tx: self.to_write_tx.clone(),
            stream_id: stream_id,
            out_window: out_window,
            stream: stream,
        }
    }

    pub fn pump_stream_to_write_loop(
        &self,
        stream_id: StreamId,
        stream: DataOrHeadersWithFlagStream,
        out_window: window_size::StreamOutWindowReceiver)
    {
        let stream = stream.catch_unwind();
        self.exec.execute(Box::new(self.new_pump_stream_to_write_loop(
            stream_id,
            stream,
            out_window)));
    }

    pub fn increase_in_window(&mut self, stream_id: StreamId, increase: u32) -> result::Result<()> {
        if let Some(mut stream) = self.streams.get_mut(stream_id) {
            if let Err(_) = stream.stream().in_window_size.try_increase(increase) {
                return Err(error::Error::Other("in window overflow"));
            }
        } else {
            return Ok(());
        };

        let window_update = WindowUpdateFrame::for_stream(stream_id, increase);
        self.send_directly_to_network(DirectlyToNetworkFrame::WindowUpdate(window_update))?;
        
        Ok(())
    }
}


pub trait ConnInner : Sized + 'static {
    type Types : Types;

    fn process_headers(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<Self::Types>>>;
}


pub fn create_loops<T, W, R>(
    conn_data: ConnData<T>,
    write: WriteHalf<W>,
    read: ReadHalf<R>,
    write_requests: HttpFutureStreamSend<T::ToWriteMessage>,
    command_requests: HttpFutureStreamSend<T::CommandMessage>,
)
-> impl Future<Item=(), Error=error::Error>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
        W : AsyncWrite + Send + 'static,
        R : AsyncRead + Send + 'static,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
        WriteLoop<W, T> : WriteLoopCustom<Types=T>,
        ReadLoop<R, T> : ReadLoopCustom<Types=T>,
        CommandLoop<T> : CommandLoopCustom<Types=T>,
{
    let conn_data = RcMut::new(conn_data);

    let framed_read = HttpFramedJoinContinuationRead::new(read);
    let framed_write = HttpFramedWrite::new(write);

    let write_loop = WriteLoop {
        framed_write,
        inner: conn_data.clone(),
        requests: write_requests,
    };
    let read_loop = ReadLoop {
        framed_read,
        inner: conn_data.clone(),
    };
    let command_loop = CommandLoop {
        inner: conn_data.clone(),
        requests: command_requests,
    };

    let mut all_loops = AllLoops {
        write_loop,
        read_loop,
        command_loop,
    };

    future::poll_fn(move || all_loops.poll())
}

struct AllLoops<T, W, R>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
        W : AsyncWrite + Send + 'static,
        R : AsyncRead + Send + 'static,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
        WriteLoop<W, T> : WriteLoopCustom<Types=T>,
        ReadLoop<R, T> : ReadLoopCustom<Types=T>,
        CommandLoop<T> : CommandLoopCustom<Types=T>,
{
    write_loop: WriteLoop<W, T>,
    read_loop: ReadLoop<R, T>,
    command_loop: CommandLoop<T>,
}

impl<T, W, R> AllLoops<T, W, R>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
        W : AsyncWrite + Send + 'static,
        R : AsyncRead + Send + 'static,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
        WriteLoop<W, T> : WriteLoopCustom<Types=T>,
        ReadLoop<R, T> : ReadLoopCustom<Types=T>,
        CommandLoop<T> : CommandLoopCustom<Types=T>,
{
    fn poll(&mut self) -> Poll<(), error::Error> {
        let write_ready = self.write_loop.poll_write()? != Async::NotReady;
        let read_ready = self.read_loop.read_process_frame()? != Async::NotReady;
        let command_ready = self.command_loop.poll_command()? != Async::NotReady;
        Ok(if write_ready || read_ready || command_ready {
            Async::Ready(())
        } else {
            Async::NotReady
        })
    }
}
