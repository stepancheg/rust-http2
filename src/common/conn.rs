use std::collections::HashMap;

use tokio_core::reactor;

use error;
use result;

use solicit::frame::settings::HttpSettings;
use solicit::frame::*;
use solicit::session::StreamState;
use solicit::session::StreamStateIdleOrClosed;
use solicit::DEFAULT_SETTINGS;

use super::closed_streams::*;
use super::conf::*;
use super::stream::*;
use super::stream_map::*;
use super::types::*;
use super::window_size;

pub use resp::Response;

use client_died_error_holder::ConnDiedType;
use client_died_error_holder::SomethingDiedErrorHolder;
use codec::http_decode_read::HttpDecodeRead;
use codec::queued_write::QueuedWrite;
use common::conn_command_channel::ConnCommandReceiver;
use common::conn_command_channel::ConnCommandSender;
use common::conn_read::ConnReadSideCustom;
use common::conn_write::ConnWriteSideCustom;
use common::init_where::InitWhere;
use common::iteration_exit::IterationExit;
use futures::future;
use futures::sync::oneshot;
use futures::task;
use futures::Async;
use futures::Future;
use futures::Poll;
use hpack;
use solicit::stream_id::StreamId;
use solicit::WindowSize;
use std::collections::HashSet;
use tokio_io::io::ReadHalf;
use tokio_io::io::WriteHalf;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use ErrorCode;

/// Client or server fields of connection
pub trait ConnSpecific: 'static {}

/// HTTP/2 connection state with socket and streams
pub(crate) struct Conn<T: Types, I: AsyncWrite + AsyncRead + Send + 'static> {
    pub conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,

    /// Client or server specific data
    pub specific: T::ConnSpecific,
    /// Messages to be sent to write loop
    pub to_write_tx: ConnCommandSender<T>,
    /// Reactor we are using
    pub loop_handle: reactor::Handle,
    /// Known streams
    pub streams: StreamMap<T>,
    /// Last streams known to be closed by peer
    pub peer_closed_streams: ClosedStreams,

    pub last_local_stream_id: StreamId,
    pub last_peer_stream_id: StreamId,
    pub goaway_sent: Option<GoawayFrame>,
    pub goaway_received: Option<GoawayFrame>,
    pub ping_sent: Option<u64>,

    /// Tracks the size of the outbound flow control window
    pub out_window_size: WindowSize,
    /// Tracks the size of the inbound flow control window
    pub in_window_size: WindowSize,

    /// Window size from pumper point of view
    pub pump_out_window_size: window_size::ConnOutWindowSender,

    pub framed_read: HttpDecodeRead<ReadHalf<I>>,

    pub queued_write: QueuedWrite<WriteHalf<I>>,
    /// The HPACK encoder used to encode headers before sending them on this connection.
    pub encoder: hpack::Encoder,
    pub write_rx: ConnCommandReceiver<T>,

    /// Try flush outgoing connection if window allows it on the next write poll
    pub flush_conn: bool,
    /// Try flush outgoing streams if window allows it on the next write poll
    pub flush_streams: HashSet<u32>,

    /// Last known peer settings
    pub peer_settings: HttpSettings,
    /// Last our settings acknowledged
    pub our_settings_ack: HttpSettings,
    /// Last our settings sent
    pub our_settings_sent: Option<HttpSettings>,
}

impl<T: Types, I: AsyncWrite + AsyncRead + Send + 'static> Drop for Conn<T, I> {
    fn drop(&mut self) {
        for (_, stream) in self.streams.map.drain() {
            stream.conn_died(self.conn_died_error_holder.error());
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ConnStateSnapshot {
    pub in_window_size: i32,
    pub out_window_size: i32,
    pub pump_out_window_size: isize,
    pub streams: HashMap<StreamId, HttpStreamStateSnapshot>,
}

impl ConnStateSnapshot {
    pub fn single_stream(&self) -> (u32, &HttpStreamStateSnapshot) {
        let mut iter = self.streams.iter();
        let (&id, stream) = iter.next().expect("no streams");
        assert!(iter.next().is_none(), "more than one stream");
        (id, stream)
    }
}

impl<T, I> Conn<T, I>
where
    T: Types,
    Self: ConnReadSideCustom<Types = T>,
    Self: ConnWriteSideCustom<Types = T>,
    HttpStreamCommon<T>: HttpStreamData<Types = T>,
    I: AsyncWrite + AsyncRead + Send + 'static,
{
    pub fn new(
        loop_handle: reactor::Handle,
        specific: T::ConnSpecific,
        _conf: CommonConf,
        sent_settings: HttpSettings,
        to_write_tx: ConnCommandSender<T>,
        write_rx: ConnCommandReceiver<T>,
        read: ReadHalf<I>,
        write: WriteHalf<I>,
        conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,
    ) -> Self {
        let in_window_size = WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32);
        let out_window_size = WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32);

        let pump_window_size = window_size::ConnOutWindowSender::new(out_window_size.0 as u32);

        let framed_read = HttpDecodeRead::new(read);
        let queued_write = QueuedWrite::new(write);

        Conn {
            conn_died_error_holder,
            specific,
            to_write_tx,
            streams: StreamMap::new(),
            last_local_stream_id: 0,
            last_peer_stream_id: 0,
            loop_handle,
            goaway_sent: None,
            goaway_received: None,
            ping_sent: None,
            pump_out_window_size: pump_window_size,
            peer_closed_streams: ClosedStreams::new(),
            framed_read,
            queued_write,
            write_rx,
            flush_conn: false,
            encoder: hpack::Encoder::new(),
            in_window_size,
            out_window_size,
            peer_settings: DEFAULT_SETTINGS,
            our_settings_ack: DEFAULT_SETTINGS,
            our_settings_sent: Some(sent_settings),
            flush_streams: HashSet::new(),
        }
    }

    /// Allocate stream id for locally initiated stream
    pub fn next_local_stream_id(&mut self) -> StreamId {
        let id = match self.last_local_stream_id {
            0 => T::CLIENT_OR_SERVER.first_stream_id(),
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
        specific: T::HttpStreamSpecific,
    ) -> (HttpStreamRef<T>, window_size::StreamOutWindowReceiver) {
        let (out_window_sender, out_window_receiver) = self
            .pump_out_window_size
            .new_stream(self.peer_settings.initial_window_size as u32);

        let stream = HttpStreamCommon::new(
            self.our_settings_sent().initial_window_size,
            self.peer_settings.initial_window_size,
            out_window_sender,
            in_rem_content_length,
            in_message_stage,
            specific,
        );

        let stream = self.streams.insert(stream_id, stream);

        (stream, out_window_receiver)
    }

    pub fn dump_state(&self) -> ConnStateSnapshot {
        ConnStateSnapshot {
            in_window_size: self.in_window_size.0,
            out_window_size: self.out_window_size.0,
            pump_out_window_size: self.pump_out_window_size.get(),
            streams: self.streams.snapshot(),
        }
    }

    pub fn our_settings_sent(&self) -> &HttpSettings {
        if let Some(ref sent) = self.our_settings_sent {
            &sent
        } else {
            &self.our_settings_ack
        }
    }

    /// Internal helper method that decreases the outbound flow control window size.
    fn _decrease_out_window(&mut self, size: u32) -> result::Result<()> {
        // The size by which we decrease the window must be at most 2^31 - 1. We should be able to
        // reach here only after sending a DATA frame, whose payload also cannot be larger than
        // that, but we assert it just in case.
        debug_assert!(size < 0x80000000);
        self.out_window_size
            .try_decrease(size as i32)
            .map_err(|_| error::Error::WindowSizeOverflow)
    }

    /// Internal helper method that decreases the inbound flow control window size.
    pub fn decrease_in_window(&mut self, size: u32) -> result::Result<()> {
        // The size by which we decrease the window must be at most 2^31 - 1. We should be able to
        // reach here only after receiving a DATA frame, which would have been validated when
        // parsed from the raw frame to have the correct payload size, but we assert it just in
        // case.
        debug_assert!(size < 0x80000000);
        self.in_window_size
            .try_decrease_to_positive(size as i32)
            .map_err(|_| error::Error::WindowSizeOverflow)
    }

    pub fn process_dump_state(
        &mut self,
        sender: oneshot::Sender<ConnStateSnapshot>,
    ) -> result::Result<()> {
        // ignore send error, client might be already dead
        drop(sender.send(self.dump_state()));
        Ok(())
    }

    pub fn send_rst_stream(
        &mut self,
        stream_id: StreamId,
        error_code: ErrorCode,
    ) -> result::Result<()> {
        // TODO: probably notify handlers
        self.streams.remove_stream(stream_id);

        let rst_stream = RstStreamFrame::new(stream_id, error_code);
        self.send_frame_and_notify(rst_stream);
        Ok(())
    }

    pub fn send_flow_control_error(&mut self) -> result::Result<()> {
        self.send_goaway(ErrorCode::FlowControlError)
    }

    fn stream_state_idle_or_closed(&self, stream_id: StreamId) -> StreamStateIdleOrClosed {
        let last_stream_id = match T::init_where(stream_id) {
            InitWhere::Locally => self.last_local_stream_id,
            InitWhere::Peer => self.last_peer_stream_id,
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

    pub fn get_stream_maybe_send_error(
        &mut self,
        stream_id: StreamId,
        frame_type: HttpFrameType,
    ) -> result::Result<Option<HttpStreamRef<T>>> {
        let stream_state = self.stream_state(stream_id);

        match stream_state {
            StreamState::Idle => {
                let send_connection_error = match frame_type {
                    HttpFrameType::Headers
                    | HttpFrameType::Priority
                    | HttpFrameType::PushPromise => false,
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
                    HttpFrameType::WindowUpdate
                    | HttpFrameType::Priority
                    | HttpFrameType::RstStream => false,
                    _ => true,
                };

                if send_rst {
                    debug!(
                        "stream is half-closed remote: {}, sending RST_STREAM",
                        stream_id
                    );
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
                    HttpFrameType::RstStream
                    | HttpFrameType::Priority
                    | HttpFrameType::WindowUpdate => false,
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

    pub fn get_stream_for_headers_maybe_send_error(
        &mut self,
        stream_id: StreamId,
    ) -> result::Result<Option<HttpStreamRef<T>>> {
        self.get_stream_maybe_send_error(stream_id, HttpFrameType::Headers)
    }

    pub fn out_window_increased(&mut self, stream_id: Option<StreamId>) -> result::Result<()> {
        match stream_id {
            Some(stream_id) => {
                self.flush_streams.insert(stream_id);
            }
            None => self.flush_conn = true,
        }
        // Make sure writer loop is executed
        task::current().notify();
        Ok(())
    }

    /// Should we close the connection because of GOAWAY state
    pub fn end_loop(&self) -> bool {
        let goaway = self.goaway_sent.is_some() || self.goaway_received.is_some();
        let no_streams = self.streams.is_empty();
        goaway && no_streams
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
        self.send_frame_and_notify(window_update);

        Ok(())
    }

    fn poll(&mut self) -> Poll<(), error::Error> {
        // TODO: add local/peer address
        let _g = log_ndc::push(T::CONN_NDC);

        match self.process_goaway_state()? {
            IterationExit::NotReady => return Ok(Async::NotReady),
            IterationExit::ExitEarly => return Ok(Async::Ready(())),
            IterationExit::Continue => {}
        }

        let write_ready = self.poll_write()? != Async::NotReady;
        let read_ready = self.read_process_frame()? != Async::NotReady;

        Ok(if write_ready || read_ready {
            info!("connection loop complete");
            Async::Ready(())
        } else {
            Async::NotReady
        })
    }

    pub fn run(mut self) -> impl Future<Item = (), Error = error::Error> {
        future::poll_fn(move || self.poll())
    }
}
