use std::collections::HashMap;
use std::cmp;

use futures::Future;
use futures::future;
use futures;

use tokio_core::reactor;

use tokio_io::io::ReadHalf;
use tokio_io::io::WriteHalf;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;
use tokio_io::io as tokio_io;

use error;
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

use futures_misc::*;

use solicit_misc::*;
use solicit_async::*;

use super::stream::*;
use super::stream_map::*;
use super::types::*;


pub use resp::Response;


pub enum CommonToWriteMessage {
    TryFlushStream(Option<StreamId>), // flush stream when window increased or new data added
    Write(Vec<u8>),
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
        to_write_tx: futures::sync::mpsc::UnboundedSender<T::ToWriteMessage>)
            -> ConnData<T>
    {
        ConnData {
            specific: specific,
            to_write_tx: to_write_tx,
            conn: HttpConnection::new(),
            streams: StreamMap::new(),
            last_local_stream_id: 0,
            last_peer_stream_id: 0,
            loop_handle: loop_handle,
            goaway_sent: None,
            goaway_received: None,
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

    pub fn remove_stream(&mut self, stream_id: StreamId) {
        match self.streams.map.remove(&stream_id) {
            Some(_) => debug!("removed stream: {}", stream_id),
            None => debug!("incorrect request to remove stream: {}", stream_id),
        }
    }

    pub fn remove_stream_if_closed(&mut self, stream_id: StreamId) {
        if self.streams.get_mut(stream_id).expect("unknown stream").state == StreamState::Closed {
            self.remove_stream(stream_id);
        }
    }


    pub fn pop_outg_for_stream(&mut self, stream_id: StreamId) -> Option<HttpStreamCommand> {
        let r = {
            if let Some(stream) = self.streams.map.get_mut(&stream_id) {
                stream.pop_outg(&mut self.conn.out_window_size)
            } else {
                None
            }
        };
        if let Some(..) = r {
            self.remove_stream_if_closed(stream_id);
        }
        r
    }

    pub fn pop_outg_for_conn(&mut self) -> Option<(StreamId, HttpStreamCommand)> {
        // TODO: lame
        let stream_ids: Vec<StreamId> = self.streams.map.keys().cloned().collect();
        for stream_id in stream_ids {
            let r = self.pop_outg_for_stream(stream_id);
            if let Some(r) = r {
                return Some((stream_id, r));
            }
        }
        None
    }

    pub fn pop_outg_all_for_stream(&mut self, stream_id: StreamId) -> Vec<HttpStreamCommand> {
        let mut r = Vec::new();
        while let Some(p) = self.pop_outg_for_stream(stream_id) {
            r.push(p);
        }
        r
    }

    pub fn pop_outg_all_for_conn(&mut self) -> Vec<(StreamId, HttpStreamCommand)> {
        let mut r = Vec::new();
        while let Some(p) = self.pop_outg_for_conn() {
            r.push(p);
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
                const MAX_CHUNK_SIZE: usize = 8 * 1024;
                while pos < data.len() {
                    let end = cmp::min(data.len(), pos + MAX_CHUNK_SIZE);

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
        let mut send = VecSendFrame(Vec::new());
        for (stream_id, part) in self.pop_outg_all_for_conn() {
            self.write_part(&mut send, stream_id, part);
        }
        send.0
    }

    pub fn dump_state(&self) -> ConnectionStateSnapshot {
        ConnectionStateSnapshot {
            streams: self.streams.map.iter().map(|(&k, s)| (k, s.state)).collect(),
        }
    }

    fn process_headers_frame(&mut self, frame: HeadersFrame) {
        let headers = self.conn.decoder
            .decode(&frame.header_fragment())
            .map_err(error::Error::CompressionError).unwrap(); // TODO: do not panic
        let headers = Headers(headers.into_iter().map(|h| Header::new(h.0, h.1)).collect());

        let end_stream = if frame.is_end_of_stream() { EndStream::Yes } else { EndStream::No };

        self.process_headers(frame.stream_id, end_stream, headers);
    }

    fn process_settings_global(&mut self, frame: SettingsFrame) -> result::Result<()> {
        if frame.is_ack() {
            // TODO: remember which settings acked
            return Ok(());
        }

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

        self.ack_settings();

        if out_window_increased {
            self.out_window_increased(None);
        }

        Ok(())
    }
    
    fn process_stream_window_update_frame(&mut self, frame: WindowUpdateFrame) {
        {
            match self.streams.get_mut(frame.get_stream_id()) {
                Some(stream) => {
                    stream.out_window_size.try_increase(frame.increment())
                        .expect("failed to increment stream window");
                }
                None => {
                    // 6.9
                    // WINDOW_UPDATE can be sent by a peer that has sent a frame bearing the
                    // END_STREAM flag.  This means that a receiver could receive a
                    // WINDOW_UPDATE frame on a "half-closed (remote)" or "closed" stream.
                    // A receiver MUST NOT treat this as an error (see Section 5.1).
                    debug!("WINDOW_UPDATE of unknown stream: {}", frame.get_stream_id());
                }
            }
        }
        self.out_window_increased(Some(frame.get_stream_id()));
    }

    fn process_conn_window_update(&mut self, frame: WindowUpdateFrame) -> result::Result<()> {
        self.conn.out_window_size.try_increase(frame.increment())
            .expect("failed to increment conn window"); // TODO: do not panic
        self.out_window_increased(None);
        Ok(())
    }

    fn process_rst_stream_frame(&mut self, frame: RstStreamFrame) {
        let stream = self.streams.get_mut(frame.get_stream_id())
            .expect(&format!("stream not found: {}", frame.get_stream_id()));
        stream.rst(frame.error_code());
    }

    fn process_data_frame(&mut self, frame: DataFrame) {
        let stream_id = frame.get_stream_id();

        self.conn.decrease_in_window(frame.payload_len())
            .expect("failed to decrease conn win");

        let increment_conn =
            // TODO: need something better
            if self.conn.in_window_size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                let increment = DEFAULT_SETTINGS.initial_window_size;
                self.conn.in_window_size.try_increase(increment).expect("failed to increase");

                Some(increment)
            } else {
                None
            };

        let increment_stream = {
            let stream = self.streams.get_mut(frame.get_stream_id())
                .expect(&format!("stream not found: {}", frame.get_stream_id()));

            stream.in_window_size.try_decrease(frame.payload_len() as i32)
                .expect("failed to decrease stream win");

            let increment_stream =
                if stream.in_window_size.size() < (DEFAULT_SETTINGS.initial_window_size / 2) as i32 {
                    let increment = DEFAULT_SETTINGS.initial_window_size;
                    stream.in_window_size.try_increase(increment).expect("failed to increase");

                    Some(increment)
                } else {
                    None
                };

            stream.new_data_chunk(&frame.data.as_ref(), frame.is_end_of_stream());

            increment_stream
        };

        if let Some(increment_conn) = increment_conn {
            self.send_frame(WindowUpdateFrame::for_connection(increment_conn));
        }

        if let Some(increment_stream) = increment_stream {
            self.send_frame(WindowUpdateFrame::for_stream(stream_id, increment_stream));
        }
    }

    fn process_ping(&mut self, frame: PingFrame) -> result::Result<()> {
        if frame.is_ack() {
            // TODO: check we sent PING
            Ok(())
        } else {
            self.send_frame(PingFrame::new_ack(frame.opaque_data()));
            Ok(())
        }
    }

    fn process_goaway(&mut self, frame: GoawayFrame) -> result::Result<()> {
        if let Some(..) = self.goaway_received {
            return Err(error::Error::Other("GOAWAY after GOAWAY"));
        }

        self.goaway_received = Some(frame);

        Ok(())

        // TODO: After all streams end, close the connection.
        //self.common().streams
    }

    fn process_conn_frame(&mut self, frame: HttpFrameConn) -> result::Result<()> {
        match frame {
            HttpFrameConn::Settings(f) => self.process_settings_global(f),
            HttpFrameConn::PushPromise(_f) => /* TODO */ Ok(()),
            HttpFrameConn::Ping(f) => self.process_ping(f),
            HttpFrameConn::Goaway(f) => self.process_goaway(f),
            HttpFrameConn::WindowUpdate(f) => self.process_conn_window_update(f),
        }
    }

    fn process_stream_frame(&mut self, frame: HttpFrameStream) -> result::Result<()> {
        let stream_id = frame.get_stream_id();
        let end_of_stream = frame.is_end_of_stream();

        let close_local = match frame {
            HttpFrameStream::RstStream(..) => true,
            _ => false,
        };

        match frame {
            HttpFrameStream::Data(data) => self.process_data_frame(data),
            HttpFrameStream::Headers(headers) => self.process_headers_frame(headers),
            HttpFrameStream::RstStream(rst) => self.process_rst_stream_frame(rst),
            HttpFrameStream::WindowUpdate(window_update) => self.process_stream_window_update_frame(window_update),
            HttpFrameStream::Continuation(_continuation) => unreachable!("must be joined with HEADERS before that"),
        };
        if end_of_stream {
            self.close_remote(stream_id);
        }
        if close_local {
            self.close_local(stream_id);
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

    fn close_remote(&mut self, stream_id: StreamId) {
        debug!("close remote: {}", stream_id);

        // RST_STREAM and WINDOW_UPDATE can be received when we have no stream
        {
            if let Some(stream) = self.streams.get_mut(stream_id) {
                stream.close_remote();
                stream.closed_remote();
            } else {
                return;
            }
        }

        self.remove_stream_if_closed(stream_id);
    }

    fn close_local(&mut self, stream_id: StreamId) {
        debug!("close local: {}", stream_id);

        {
            if let Some(stream) = self.streams.get_mut(stream_id) {
                stream.close_local();
            } else {
                return;
            }
        }

        self.remove_stream_if_closed(stream_id);
    }

    fn send_common(&mut self, message: CommonToWriteMessage)
        where T::ToWriteMessage : From<CommonToWriteMessage>
    {
        self.to_write_tx.send(message.into())
            .expect("send to write"); // TODO: do not panic
    }

    /// Send a frame back to the network
    /// Must not be data frame
    fn send_frame<R : FrameIR>(&mut self, frame: R) {
        let mut send_buf = VecSendFrame(Vec::new());
        send_buf.send_frame(frame).unwrap();
        self.send_common(CommonToWriteMessage::Write(send_buf.0))
    }

    fn out_window_increased(&mut self, stream_id: Option<StreamId>) {
        self.send_common(CommonToWriteMessage::TryFlushStream(stream_id))
    }

    /// Sends an SETTINGS Frame with ack set to acknowledge seeing a SETTINGS frame from the peer.
    fn ack_settings(&mut self) {
        self.send_frame(SettingsFrame::new_ack());
    }

}


pub trait ConnInner : 'static {
    type Types : Types;

    fn process_headers(&mut self, stream_id: StreamId, end_stream: EndStream, headers: Headers);
}


pub struct ReadLoopData<I, T>
    where
        I : AsyncRead + 'static,
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub read: ReadHalf<I>,
    pub inner: TaskRcMut<ConnData<T>>,
}

pub struct WriteLoopData<I, T>
    where
        I : AsyncWrite + 'static,
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub write: WriteHalf<I>,
    pub inner: TaskRcMut<ConnData<T>>,
}

pub struct CommandLoopData<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStream,
{
    pub inner: TaskRcMut<ConnData<T>>,
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
        Box::new(recv_http_frame_join_cont(read)
            .map(|(read, frame)| (ReadLoopData { read: read, inner: inner }, frame)))
    }

    fn read_process_frame(self) -> HttpFuture<Self> {
        Box::new(self.recv_http_frame()
            .and_then(move |(lp, frame)| lp.process_http_frame(frame)))
    }

    pub fn run(self) -> HttpFuture<()> {
        let future = future::loop_fn(self, |lp| {
            lp.read_process_frame().map(future::Loop::Continue::<(), _>)
        });

        Box::new(future)
    }

    fn process_http_frame(self, frame: HttpFrame) -> HttpFuture<Self> {
        Box::new(future::result(self.inner.with(move |inner| {
            inner.process_http_frame(frame)
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
    pub fn write_all(self, buf: Vec<u8>) -> HttpFuture<Self> {
        let WriteLoopData { write, inner } = self;

        Box::new(tokio_io::write_all(write, buf)
            .map(move |(write, _)| WriteLoopData { write: write, inner: inner })
            .map_err(error::Error::from))
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

    pub fn process_common(self, common: CommonToWriteMessage) -> HttpFuture<Self> {
        match common {
            CommonToWriteMessage::TryFlushStream(None) => self.send_outg_conn(),
            CommonToWriteMessage::TryFlushStream(Some(stream_id)) => self.send_outg_stream(stream_id),
            CommonToWriteMessage::Write(buf) => self.write_all(buf),
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
