use std::collections::HashMap;
use std::cmp;

use futures::Future;
use futures::future;
use futures::future::Loop;
use futures::future::loop_fn;
use futures::stream::Stream;
use futures;

use tokio_core::reactor;

use tokio_io::io::ReadHalf;
use tokio_io::io::WriteHalf;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use tokio_io::io as tokio_io;

use error;
use error::ErrorCode;
use result;

use solicit::session::StreamState;
use solicit::frame::*;
use solicit::header::*;
use solicit::StreamId;
use solicit::DEFAULT_SETTINGS;
use solicit::connection::EndStream;
use solicit::connection::HttpConnection;
use solicit::connection::SendFrame;
use solicit::connection::HttpFrame;
use solicit::connection::HttpFrameType;
use solicit::frame::settings::HttpSettings;

use futures_misc::*;

use solicit_misc::*;
use solicit_async::*;

use super::stream::*;
use super::stream_map::*;
use super::types::*;
use super::conf::*;

use stream_part::*;

pub use resp::Response;

use rc_mut::*;


pub enum CommonToWriteMessage {
    TryFlushStream(Option<StreamId>), // flush stream when window increased or new data added
    Frame(HttpFrame),
    StreamEnd(StreamId, ErrorCode), // send when user provided handler completed the stream
}

pub trait ConnDataSpecific : 'static {
}


pub struct ConnData<T : Types> {
    /// Client or server specific data
    pub specific: T::ConnDataSpecific,
    /// Messages to be sent to write loop
    pub to_write_tx: futures::sync::mpsc::UnboundedSender<T::ToWriteMessage>,
    /// Reactor we are using
    pub loop_handle: reactor::Handle,
    /// Connection state
    pub conn: HttpConnection,
    /// Known streams
    pub streams: StreamMap<T>,
    pub last_local_stream_id: StreamId,
    pub last_peer_stream_id: StreamId,
    pub goaway_sent: Option<GoawayFrame>,
    pub goaway_received: Option<GoawayFrame>,
    pub ping_sent: Option<u64>,
}


#[derive(Debug)]
pub struct ConnectionStateSnapshot {
    pub streams: HashMap<StreamId, StreamState>,
}




impl<T : Types> ConnData<T>
    where
        Self : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStream,
{

    pub fn new(
        loop_handle: reactor::Handle,
        specific: T::ConnDataSpecific,
        _conf: CommonConf,
        sent_settings: HttpSettings,
        to_write_tx: futures::sync::mpsc::UnboundedSender<T::ToWriteMessage>)
            -> ConnData<T>
    {
        let mut conn = HttpConnection::new();
        conn.our_settings_sent = Some(sent_settings);

        ConnData {
            specific: specific,
            to_write_tx: to_write_tx,
            conn: conn,
            streams: StreamMap::new(),
            last_local_stream_id: 0,
            last_peer_stream_id: 0,
            loop_handle: loop_handle,
            goaway_sent: None,
            goaway_received: None,
            ping_sent: None,
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

    fn write_part(&mut self, target: &mut VecSendFrame, stream_id: StreamId, part: HttpStreamCommand) {
        match part {
            HttpStreamCommand::Data(data, end_stream) => {
                // if client requested end of stream,
                // we must send at least one frame with end stream flag
                if end_stream == EndStream::Yes && data.len() == 0 {
                    // probably should send RST_STREAM
                    let mut frame = DataFrame::with_data(stream_id, Vec::new());
                    frame.set_flag(DataFlag::EndStream);

                    debug!("sending frame {:?}", frame);

                    return target.send_frame(frame).unwrap();
                }

                let mut pos = 0;
                let max_frame_size = self.conn.peer_settings.max_frame_size as usize;
                while pos < data.len() {
                    let end = cmp::min(data.len(), pos + max_frame_size);

                    let end_stream_in_frame =
                        if end == data.len() && end_stream == EndStream::Yes {
                            EndStream::Yes
                        } else {
                            EndStream::No
                        };

                    let mut frame = DataFrame::with_data(stream_id, &data[pos..end]);
                    if end_stream_in_frame == EndStream::Yes {
                        frame.set_flag(DataFlag::EndStream);
                    }

                    debug!("sending frame {:?}", frame);

                    target.send_frame(frame).unwrap();

                    pos = end;
                }
            }
            HttpStreamCommand::Headers(headers, end_stream) => {
                let headers_fragment = self
                    .conn.encoder.encode(headers.0.iter().map(|h| (h.name(), h.value())));

                // For now, sending header fragments larger than 16kB is not supported
                // (i.e. the encoded representation cannot be split into CONTINUATION
                // frames).
                let mut frame = HeadersFrame::new(headers_fragment, stream_id);
                frame.set_flag(HeadersFlag::EndHeaders);

                if end_stream == EndStream::Yes {
                    frame.set_flag(HeadersFlag::EndStream);
                }

                debug!("sending frame {:?}", frame);

                target.send_frame(frame).unwrap();
            }
            HttpStreamCommand::Rst(error_code) => {
                let frame = RstStreamFrame::new(stream_id, error_code);

                debug!("sending frame {:?}", frame);

                target.send_frame(frame).unwrap();
            }
        }
    }

    pub fn pop_outg_all_for_stream_bytes(&mut self, stream_id: StreamId) -> Vec<u8> {
        let mut send = VecSendFrame(Vec::new());
        for part in self.pop_outg_all_for_stream(stream_id) {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    pub fn pop_outg_all_for_conn_bytes(&mut self) -> Vec<u8> {
        // TODO: maintain own limits of out window
        let mut send = VecSendFrame(Vec::new());
        for (stream_id, part) in self.pop_outg_all_for_conn() {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    pub fn dump_state(&self) -> ConnectionStateSnapshot {
        ConnectionStateSnapshot {
            streams: self.streams.snapshot(),
        }
    }

    fn process_headers_frame(&mut self, self_rc: RcMut<Self>, frame: HeadersFrame) -> result::Result<Option<HttpStreamRef<T>>> {
        let headers = self.conn.decoder
            .decode(&frame.header_fragment())
            .map_err(error::Error::CompressionError).unwrap(); // TODO: do not panic
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
            if let HttpSetting::InitialWindowSize(new_size) = setting {
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
                        s.out_window_size.0 += delta;
                    }

                    if !self.streams.map.is_empty() && delta > 0 {
                        out_window_increased = true;
                    }
                }
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
        self.out_window_increased(Some(frame.get_stream_id()))?;

        match self.streams.get_mut(frame.get_stream_id()) {
            Some(mut stream) => {
                // TODO: stream error, not conn error
                stream.stream().out_window_size.try_increase(frame.increment())
                    .map_err(|()| error::Error::Other("failed to increment stream window"))?;
                Ok(Some(stream))
            }
            None => {
                // 6.9
                // WINDOW_UPDATE can be sent by a peer that has sent a frame bearing the
                // END_STREAM flag.  This means that a receiver could receive a
                // WINDOW_UPDATE frame on a "half-closed (remote)" or "closed" stream.
                // A receiver MUST NOT treat this as an error (see Section 5.1).
                debug!("WINDOW_UPDATE of unknown stream: {}", frame.get_stream_id());
                Ok(None)
            }
        }
    }

    fn process_conn_window_update(&mut self, frame: WindowUpdateFrame) -> result::Result<()> {
        self.conn.out_window_size.try_increase(frame.increment())
            .map_err(|()| error::Error::Other("failed to increment conn window"))?;
        self.out_window_increased(None)
    }

    fn process_rst_stream_frame(&mut self, frame: RstStreamFrame)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        if let Some(stream) = self.streams.get_mut(frame.get_stream_id()) {
            stream.rst_remove(frame.error_code());
        } else {
            warn!("RST_STREAM on non-existent stream: {}", frame.stream_id);
        }

        Ok(None)
    }

    fn get_stream_or_send_stream_closed(&mut self, stream_id: StreamId)
        -> result::Result<Option<HttpStreamRef<T>>>
    {
        // Another day in endless bitter war against borrow checker
        if self.streams.get_mut(stream_id).is_some() {
            return Ok(Some(self.streams.get_mut(stream_id).unwrap()));
        }

        debug!("stream not found: {}, sending RST_STREAM", stream_id);
        self.send_frame(RstStreamFrame::new(stream_id, ErrorCode::StreamClosed))?;

        return Ok(None);
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

        let increment_stream = {
            // If a DATA frame is received whose stream is not in "open" or
            // "half-closed (local)" state, the recipient MUST respond with
            // a stream error (Section 5.4.2) of type STREAM_CLOSED.
            let mut stream = match self.get_stream_or_send_stream_closed(frame.get_stream_id())? {
                Some(stream) => stream,
                None => {
                    return Ok(None);
                }
            };

            stream.stream().in_window_size.try_decrease_to_positive(frame.payload_len() as i32)
                .map_err(|()| error::Error::CodeError(ErrorCode::FlowControlError))?;

            let increment_stream =
                if stream.stream().in_window_size.size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                    let increment = DEFAULT_SETTINGS.initial_window_size;
                    stream.stream().in_window_size.try_increase(increment)
                        .map_err(|()| error::Error::Other("failed to increase window size"))?;

                    Some(increment)
                } else {
                    None
                };

            stream.stream().new_data_chunk(&frame.data.as_ref(), frame.is_end_of_stream());

            increment_stream
        };

        if let Some(increment_conn) = increment_conn {
            self.send_frame(WindowUpdateFrame::for_connection(increment_conn))?;
        }

        if let Some(increment_stream) = increment_stream {
            self.send_frame(WindowUpdateFrame::for_stream(stream_id, increment_stream))?;
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
                Err(error::Error::Other("PING ACK without PING"))
            }
        } else {
            self.send_frame(PingFrame::new_ack(frame.opaque_data()))
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

        let stream = match frame {
            HttpFrameStream::Data(data) => self.process_data_frame(data)?,
            HttpFrameStream::Headers(headers) => self.process_headers_frame(self_rc, headers)?,
            HttpFrameStream::Priority(priority) => self.process_priority_frame(priority)?,
            HttpFrameStream::RstStream(rst) => self.process_rst_stream_frame(rst)?,
            HttpFrameStream::PushPromise(_f) => unimplemented!(),
            HttpFrameStream::WindowUpdate(window_update) => self.process_stream_window_update_frame(window_update)?,
            HttpFrameStream::Continuation(_continuation) => unreachable!("must be joined with HEADERS before that"),
        };

        if let Some(mut stream) = stream {
            if end_of_stream {
                stream.stream().close_remote();
            }
            stream.remove_if_closed();
        }

        Ok(())
    }

    fn process_http_frame(&mut self, self_rc: RcMut<Self>, frame: HttpFrame) -> result::Result<()> {
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
        self.to_write_tx.send(message.into())
            .map_err(|_| error::Error::Other("write loop died"))
    }

    /// Schedule a write for HTTP frame
    /// Must not be data frame
    fn send_frame<F : Into<HttpFrame>>(&mut self, frame: F) -> result::Result<()> {
        let frame = frame.into();
        assert!(frame.frame_type() != HttpFrameType::Data);
        self.send_common(CommonToWriteMessage::Frame(frame))
    }

    fn out_window_increased(&mut self, stream_id: Option<StreamId>) -> result::Result<()> {
        self.send_common(CommonToWriteMessage::TryFlushStream(stream_id))
    }

    /// Sends an SETTINGS Frame with ack set to acknowledge seeing a SETTINGS frame from the peer.
    fn ack_settings(&mut self) -> result::Result<()> {
        self.send_frame(SettingsFrame::new_ack())
    }

    /// Should we close the connection because of GOAWAY state
    pub fn end_loop(&self) -> bool {
        let goaway = self.goaway_sent.is_some() || self.goaway_received.is_some();
        let no_streams = self.streams.is_empty();
        goaway && no_streams
    }

    pub fn pump_stream_to_write_loop(
        &mut self,
        self_rc: RcMut<Self>,
        stream_id: StreamId,
        stream: HttpPartStream,
        ready_to_write: Latch)
    {
        let to_write_tx_2 = self.to_write_tx.clone();

        let stream = stream.catch_unwind();

        let ready_to_write = ready_to_write.map_err(|()| {
            error::Error::Other("error from latch; stream must be closed")
        });

        let future = loop_fn((ready_to_write, stream, self.to_write_tx.clone(), self_rc), move |(ready_to_write, stream, to_write_tx, self_rc)| {
            // Only poll user-provided callback when out window is available
            ready_to_write.into_future().map_err(|(e, _)| e)
                .and_then(move |(o, ready_to_write)| {
                    if let None::<()> = o {
                        unreachable!();
                    }

                    stream.into_future().map_err(|(e, _)| e)
                        .and_then(move |(part_opt, stream)| {
                            let (cont, to_write_tx) = self_rc.with(move |conn| {
                                let cont = if let Some(mut stream) = conn.streams.get_mut(stream_id) {
                                    if !stream.stream().state.is_closed_local() {
                                        match part_opt {
                                            Some(part) => {
                                                stream.stream().outgoing.push_back_part(part);
                                                stream.check_ready_to_write(&mut conn.conn.out_window_size);
                                                true
                                            }
                                            None => {
                                                stream.stream().outgoing.close(ErrorCode::NoError);
                                                false
                                            }
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };
                                if let Err(e) = to_write_tx.send(CommonToWriteMessage::TryFlushStream(Some(stream_id)).into()) {
                                    warn!("failed to write to channel, probably connection is closed: {:?}", e);
                                }
                                (cont, to_write_tx)
                            });

                            future::ok(if cont {
                                Loop::Continue((ready_to_write, stream, to_write_tx, self_rc))
                            } else {
                                Loop::Break(())
                            })
                        })
                })
        });

        let future = future.then(move |r| {
            let error_code =
                match r {
                    Ok(()) => ErrorCode::NoError,
                    Err(e) => {
                        warn!("handler stream error: {:?}", e);
                        ErrorCode::InternalError
                    }
                };
            if let Err(e) = to_write_tx_2.send(CommonToWriteMessage::StreamEnd(stream_id, error_code).into()) {
                warn!("failed to write to channel, probably connection is closed: {}", e);
            }
            Ok(())
        });

        self.loop_handle.spawn(future);
    }
}


pub trait ConnInner : Sized + 'static {
    type Types : Types;

    fn process_headers(&mut self, self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<Self::Types>>>;

    fn goaway_received(&mut self, stream_id: StreamId, raw_error_code: u32);
}


pub struct ReadLoopData<I, T>
    where
        I : AsyncRead + 'static,
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub read: ReadHalf<I>,
    pub inner: RcMut<ConnData<T>>,
}

pub struct WriteLoopData<I, T>
    where
        I : AsyncWrite + 'static,
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub write: WriteHalf<I>,
    pub inner: RcMut<ConnData<T>>,
}

pub struct CommandLoopData<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub inner: RcMut<ConnData<T>>,
}


impl<I, T> ReadLoopData<I, T>
    where
        I : AsyncRead + Send + 'static,
        T : Types,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStream<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(self) -> HttpFuture<(Self, HttpFrame)> {
        let ReadLoopData { read, inner } = self;

        // TODO: use our settings
        let max_frame_size = inner.with(|_| DEFAULT_SETTINGS.max_frame_size);

        Box::new(recv_http_frame_join_cont(read, max_frame_size)
            .map(|(read, frame)| (ReadLoopData { read: read, inner: inner }, frame)))
    }

    fn read_process_frame(self) -> HttpFuture<Self> {
        Box::new(self.recv_http_frame()
            .and_then(move |(lp, frame)| lp.process_http_frame(frame)))
    }

    fn loop_iter(self) -> HttpFuture<Loop<(), Self>> {
        if self.inner.with(|inner| inner.end_loop()) {
            return Box::new(future::err(error::Error::Other("GOAWAY")));
            //return Box::new(future::ok(Loop::Break(())));
        }

        Box::new(self.read_process_frame().map(Loop::Continue))
    }

    pub fn run(self) -> HttpFuture<()> {
        Box::new(loop_fn(self, Self::loop_iter))
    }

    fn process_http_frame(self, frame: HttpFrame) -> HttpFuture<Self> {
        let inner_rc = self.inner.clone();

        Box::new(future::result(self.inner.with(move |inner| {
            inner.process_http_frame(inner_rc, frame)
        }).map(|()| self)))
    }

}

impl<I, T> WriteLoopData<I, T>
    where
        I : AsyncWrite + Send + 'static,
        T : Types,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStream<Types=T>,
{
    fn write_all(self, buf: Vec<u8>) -> HttpFuture<Self> {
        let WriteLoopData { write, inner } = self;

        Box::new(tokio_io::write_all(write, buf)
            .map(move |(write, _)| WriteLoopData { write: write, inner: inner })
            .map_err(error::Error::from))
    }

    fn write_frame(self, frame: HttpFrame) -> HttpFuture<Self> {
        debug!("send {:?}", frame);

        let mut send_buf = VecSendFrame(Vec::new());
        send_buf.send_frame(frame).unwrap();

        self.write_all(send_buf.0)
    }

    fn with_inner<G, R>(&self, f: G) -> R
        where G : FnOnce(&mut ConnData<T>) -> R
    {
        self.inner.with(f)
    }

    pub fn send_outg_stream(self, stream_id: StreamId) -> HttpFuture<Self> {
        let bytes = self.with_inner(|inner| {
            inner.pop_outg_all_for_stream_bytes(stream_id)
        });

        self.write_all(bytes)
    }

    fn send_outg_conn(self) -> HttpFuture<Self> {
        let bytes = self.with_inner(|inner| {
            inner.pop_outg_all_for_conn_bytes()
        });

        self.write_all(bytes)
    }

    fn process_stream_end(self, stream_id: StreamId, error_code: ErrorCode) -> HttpFuture<Self> {
        let stream_id = self.inner.with(move |inner| {
            let stream = inner.streams.get_mut(stream_id);
            if let Some(mut stream) = stream {
                stream.stream().outgoing.close(error_code);
                Some(stream_id)
            } else {
                None
            }
        });
        if let Some(stream_id) = stream_id {
            self.send_outg_stream(stream_id)
        } else {
            Box::new(futures::finished(self))
        }
    }

    pub fn process_common(self, common: CommonToWriteMessage) -> HttpFuture<Self> {
        match common {
            CommonToWriteMessage::TryFlushStream(None) => self.send_outg_conn(),
            CommonToWriteMessage::TryFlushStream(Some(stream_id)) => self.send_outg_stream(stream_id),
            CommonToWriteMessage::Frame(frame) => self.write_frame(frame),
            CommonToWriteMessage::StreamEnd(stream_id, error_code) => self.process_stream_end(stream_id, error_code),
        }
    }
}

impl<T> CommandLoopData<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
}
