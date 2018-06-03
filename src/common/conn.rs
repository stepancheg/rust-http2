use std::collections::HashMap;

use futures::sync::mpsc::UnboundedSender;

use tokio_core::reactor;

use exec::Executor;
use exec::CpuPoolOption;

use error;
use error::ErrorCode;
use result;

use solicit::frame::*;
use solicit::StreamId;
use solicit::DEFAULT_SETTINGS;
use solicit::connection::HttpFrameType;
use solicit::frame::settings::HttpSettings;
use solicit::session::StreamState;
use solicit::session::StreamStateIdleOrClosed;

use super::stream::*;
use super::stream_map::*;
use super::closed_streams::*;
use super::types::*;
use super::conf::*;
use super::pump_stream_to_write_loop::PumpStreamToWrite;
use super::stream_from_network::StreamFromNetwork;
use super::stream_queue_sync::StreamQueueSyncReceiver;
use super::window_size;
use super::stream_queue_sync::stream_queue_sync;


pub use resp::Response;

use client_died_error_holder::ClientDiedErrorHolder;
use client_died_error_holder::ClientConnDiedType;
use data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use codec::http_framed_read::HttpFramedJoinContinuationRead;
use solicit_async::HttpFutureStreamSend;
use futures::future;
use futures::Future;
use futures::Poll;
use futures::Async;
use futures::sync::oneshot;
use tokio_io::io::ReadHalf;
use tokio_io::io::WriteHalf;
use common::conn_read::ConnReadSideCustom;
use common::conn_command::ConnCommandSideCustom;
use common::conn_write::ConnWriteSideCustom;
use hpack;
use solicit::WindowSize;
use std::collections::HashSet;
use futures::task;
use common::iteration_exit::IterationExit;
use codec::queued_write::QueuedWrite;


/// Client or server fields of connection
pub trait ConnSpecific : 'static {
}


/// HTTP/2 connection state with socket and streams
pub struct Conn<T : Types> {
    pub conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,

    /// Client or server specific data
    pub specific: T::ConnDataSpecific,
    /// Messages to be sent to write loop
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
    /// Reactor we are using
    pub loop_handle: reactor::Handle,
    /// Executor which drives requests on client and responses on server
    pub exec: Box<Executor>,
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

    pub command_rx: HttpFutureStreamSend<T::CommandMessage>,

    /// Tracks the size of the outbound flow control window
    pub out_window_size: WindowSize,
    /// Tracks the size of the inbound flow control window
    pub in_window_size: WindowSize,

    pub framed_read: HttpFramedJoinContinuationRead<ReadHalf<T::Io>>,
    /// HPACK decoder used to decode incoming headers before passing them on to the session.
    pub decoder: hpack::Decoder<'static>,

    pub queued_write: QueuedWrite<WriteHalf<T::Io>>,
    /// The HPACK encoder used to encode headers before sending them on this connection.
    pub encoder: hpack::Encoder<'static>,
    pub write_rx: HttpFutureStreamSend<T::ToWriteMessage>,

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


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ConnStateSnapshot {
    pub in_window_size: i32,
    pub out_window_size: i32,
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



impl<T> Conn<T>
    where
        T : Types,
        Self : ConnReadSideCustom<Types=T>,
        Self : ConnWriteSideCustom<Types=T>,
        Self : ConnCommandSideCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    pub fn new(
        loop_handle: reactor::Handle,
        exec: CpuPoolOption,
        specific: T::ConnDataSpecific,
        _conf: CommonConf,
        sent_settings: HttpSettings,
        to_write_tx: UnboundedSender<T::ToWriteMessage>,
        command_rx: HttpFutureStreamSend<T::CommandMessage>,
        write_rx: HttpFutureStreamSend<T::ToWriteMessage>,
        read: ReadHalf<T::Io>,
        write: WriteHalf<T::Io>,
        conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>)
            -> Conn<T>
    {
        let in_window_size = WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32);
        let out_window_size = WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32);

        let pump_window_size = window_size::ConnOutWindowSender::new(out_window_size.0 as u32);

        let framed_read = HttpFramedJoinContinuationRead::new(read);
        let queued_write = QueuedWrite::new(write);

        Conn {
            conn_died_error_holder,
            specific,
            to_write_tx,
            streams: StreamMap::new(),
            last_local_stream_id: 0,
            last_peer_stream_id: 0,
            exec: exec.make_executor(&loop_handle),
            loop_handle,
            goaway_sent: None,
            goaway_received: None,
            ping_sent: None,
            pump_out_window_size: pump_window_size,
            peer_closed_streams: ClosedStreams::new(),
            command_rx,
            framed_read,
            queued_write,
            write_rx,
            flush_conn: false,
            decoder: hpack::Decoder::new(),
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

        let in_window_size = self.our_settings_sent().initial_window_size;

        let stream_from_network = self.new_stream_from_network(
            inc_rx,
            stream_id,
            in_window_size);

        let (out_window_sender, out_window_receiver) =
            self.pump_out_window_size.new_stream(self.peer_settings.initial_window_size as u32);

        let stream = HttpStreamCommon::new(
            in_window_size,
            self.peer_settings.initial_window_size,
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
            rx,
            stream_id,
            to_write_tx: self.to_write_tx.clone(),
            in_window_size,
        }
    }


    pub fn dump_state(&self) -> ConnStateSnapshot {
        ConnStateSnapshot {
            in_window_size: self.in_window_size.0,
            out_window_size: self.out_window_size.0,
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

    pub fn process_dump_state(&mut self, sender: oneshot::Sender<ConnStateSnapshot>)
        -> result::Result<()>
    {
        // ignore send error, client might be already dead
        drop(sender.send(self.dump_state()));
        Ok(())
    }

    pub fn send_rst_stream(&mut self, stream_id: StreamId, error_code: ErrorCode)
        -> result::Result<()>
    {
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

    pub fn out_window_increased(&mut self, stream_id: Option<StreamId>) -> result::Result<()> {
        match stream_id {
            Some(stream_id) => { self.flush_streams.insert(stream_id); },
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

    pub fn new_pump_stream_to_write_loop(
        &self,
        stream_id: StreamId,
        stream: DataOrHeadersWithFlagStream,
        out_window: window_size::StreamOutWindowReceiver)
        -> PumpStreamToWrite<T>
    {
        let stream = stream.catch_unwind();
        PumpStreamToWrite {
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
        self.send_frame_and_notify(window_update);

        Ok(())
    }

    fn poll(&mut self) -> Poll<(), error::Error> {
        match self.process_goaway_state()? {
            IterationExit::NotReady => return Ok(Async::NotReady),
            IterationExit::ExitEarly => return Ok(Async::Ready(())),
            IterationExit::Continue => {}
        }

        let write_ready = self.poll_write()? != Async::NotReady;
        let read_ready = self.read_process_frame()? != Async::NotReady;
        let command_ready = self.poll_command()? != Async::NotReady;

        Ok(if write_ready || read_ready || command_ready {
            info!("connection loop complete");
            Async::Ready(())
        } else {
            Async::NotReady
        })
    }

    pub fn run(mut self) -> impl Future<Item=(), Error=error::Error> {
        future::poll_fn(move || self.poll())
    }
}
