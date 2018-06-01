use tokio_io::io::WriteHalf;

use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use rc_mut::RcMut;
use solicit::connection::HttpFrame;
use solicit::StreamId;

use data_or_headers_with_flag::DataOrHeadersWithFlag;

use error;
use ErrorCode;
use solicit::frame::RstStreamFrame;
use solicit::frame::GoawayFrame;
use solicit::frame::WindowUpdateFrame;
use solicit::frame::PingFrame;
use solicit::frame::SettingsFrame;
use codec::http_framed_write::HttpFramedWrite;
use result;
use futures::Poll;
use solicit_async::HttpFutureStreamSend;
use futures::Async;


pub enum DirectlyToNetworkFrame {
    RstStream(RstStreamFrame),
    GoAway(GoawayFrame),
    WindowUpdate(WindowUpdateFrame),
    Ping(PingFrame),
    Settings(SettingsFrame),
}

impl DirectlyToNetworkFrame {
    pub fn into_http_frame(self) -> HttpFrame {
        match self {
            DirectlyToNetworkFrame::RstStream(f) => f.into(),
            DirectlyToNetworkFrame::GoAway(f) => f.into(),
            DirectlyToNetworkFrame::WindowUpdate(f) => f.into(),
            DirectlyToNetworkFrame::Ping(f) => f.into(),
            DirectlyToNetworkFrame::Settings(f) => f.into(),
        }
    }
}


pub struct WriteLoop<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub framed_write: HttpFramedWrite<WriteHalf<T::Io>>,
    pub inner: RcMut<ConnData<T>>,
    pub requests: HttpFutureStreamSend<T::ToWriteMessage>,
}

pub trait WriteLoopCustom {
    type Types : Types;

    fn process_message(&mut self, message: <Self::Types as Types>::ToWriteMessage) -> result::Result<()>;
}

impl<T> WriteLoop<T>
    where
        T : Types,
        Self : WriteLoopCustom<Types=T>,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    pub fn poll_flush(&mut self) -> Poll<(), error::Error> {
        self.framed_write.poll_flush()
    }

    fn with_inner<G, R>(&self, f: G) -> R
        where G : FnOnce(&mut ConnData<T>) -> R
    {
        self.inner.with(f)
    }

    pub fn buffer_outg_stream(&mut self, stream_id: StreamId) {
        let bytes = self.with_inner(|inner| {
            inner.pop_outg_all_for_stream_bytes(stream_id)
        });

        self.framed_write.buffer(bytes.into());
    }

    fn buffer_outg_conn(&mut self) {
        let bytes = self.with_inner(|inner| {
            inner.pop_outg_all_for_conn_bytes()
        });

        self.framed_write.buffer(bytes.into());
    }

    fn process_stream_end(&mut self, stream_id: StreamId, error_code: ErrorCode) -> result::Result<()> {
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
            self.buffer_outg_stream(stream_id);
        }
        Ok(())
    }

    fn process_stream_enqueue(&mut self, stream_id: StreamId, part: DataOrHeadersWithFlag) -> result::Result<()> {
        let stream_id = self.inner.with(move |inner| {
            let stream = inner.streams.get_mut(stream_id);
            if let Some(mut stream) = stream {
                stream.stream().outgoing.push_back_part(part);
                Some(stream_id)
            } else {
                None
            }
        });
        if let Some(stream_id) = stream_id {
            self.buffer_outg_stream(stream_id);
        }
        Ok(())
    }

    fn increase_in_window(&mut self, stream_id: StreamId, increase: u32)
        -> result::Result<()>
    {
        self.inner.with(move |inner| {
            inner.increase_in_window(stream_id, increase)
        })
    }

    pub fn process_common_message(&mut self, common: CommonToWriteMessage) -> result::Result<()> {
        match common {
            CommonToWriteMessage::TryFlushStream(None) => {
                self.buffer_outg_conn();
            },
            CommonToWriteMessage::TryFlushStream(Some(stream_id)) => {
                self.buffer_outg_stream(stream_id);
            },
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
            CommonToWriteMessage::CloseConn => {
                return Err(error::Error::Other("close connection"));
            }
        }
        Ok(())
    }

    pub fn poll_write(&mut self) -> Poll<(), error::Error> {
        loop {
            if let Async::NotReady = self.poll_flush()? {
                return Ok(Async::NotReady);
            }

            let message = match self.requests.poll()? {
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
    TryFlushStream(Option<StreamId>), // flush stream when window increased or new data added
    IncreaseInWindow(StreamId, u32),
    Frame(DirectlyToNetworkFrame),    // write frame immediately to the network
    StreamEnqueue(StreamId, DataOrHeadersWithFlag),
    StreamEnd(StreamId, ErrorCode),   // send when user provided handler completed the stream
    CloseConn,
}
