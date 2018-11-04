use bytes::Bytes;
use common::conn_command_channel::ConnCommandSender;
use common::conn_write::CommonToWriteMessage;
use common::types::Types;
use common::window_size::StreamOutWindowReceiver;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use error;
use futures::future;
use futures::future::Future;
use futures::Async;
use futures::Poll;
use futures::Stream;
use solicit::stream_id::StreamId;
use std::sync::Arc;
use ErrorCode;
use Headers;
use HttpStreamAfterHeaders;
use StreamDead;

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum SenderState {
    ExpectingHeaders,
    ExpectingBodyOrTrailers,
    Done,
}

#[derive(Debug)]
pub enum SendError {
    ConnectionDied(Arc<error::Error>),
    IncorrectState(SenderState),
}

struct CanSendData<T: Types> {
    write_tx: ConnCommandSender<T>,
    out_window: StreamOutWindowReceiver,
    seen_headers: bool,
}

/// Shared implementation of sender for client and server
pub(crate) struct CommonSender<T: Types> {
    state: Option<CanSendData<T>>,
    stream_id: StreamId,
}

impl<T: Types> CommonSender<T> {
    pub fn new(
        stream_id: StreamId,
        write_tx: ConnCommandSender<T>,
        out_window: StreamOutWindowReceiver,
        seen_headers: bool,
    ) -> Self {
        CommonSender {
            state: Some(CanSendData {
                write_tx,
                out_window,
                seen_headers,
            }),
            stream_id,
        }
    }

    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        match self.state {
            Some(ref mut state) => state.out_window.poll(),
            // TODO: different error
            None => Ok(Async::Ready(())),
        }
    }

    pub fn block_wait(&mut self) -> Result<(), StreamDead> {
        future::poll_fn(|| self.poll()).wait()
    }

    fn get_can_send(&mut self) -> Result<&mut CanSendData<T>, SendError> {
        match self.state {
            Some(ref mut state) => Ok(state),
            None => Err(SendError::IncorrectState(SenderState::Done)),
        }
    }

    pub fn state(&self) -> SenderState {
        match self.state {
            Some(CanSendData {
                seen_headers: true, ..
            }) => SenderState::ExpectingBodyOrTrailers,
            Some(CanSendData {
                seen_headers: false,
                ..
            }) => SenderState::ExpectingHeaders,
            None => SenderState::Done,
        }
    }

    pub fn send_common(&mut self, message: CommonToWriteMessage) -> Result<(), SendError> {
        // TODO: why client died?
        self.get_can_send()?
            .write_tx
            .unbounded_send(message.into())
            .map_err(|e| SendError::ConnectionDied(Arc::new(e)))
    }

    fn send_data_impl(&mut self, data: Bytes, last: bool) -> Result<(), SendError> {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()));
        }
        let stream_id = self.stream_id;
        self.get_can_send()?.out_window.decrease(data.len());
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                last,
            },
        ))?;
        if last {
            self.state.take();
        }
        Ok(())
    }

    pub fn send_data(&mut self, data: Bytes) -> Result<(), SendError> {
        self.send_data_impl(data, false)
    }

    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> Result<(), SendError> {
        self.send_data_impl(data, true)
    }

    pub fn send_headers(&mut self, headers: Headers) -> Result<(), SendError> {
        if self.state() != SenderState::ExpectingHeaders {
            return Err(SendError::IncorrectState(self.state()));
        }
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(headers),
                last: false,
            },
        ))?;
        self.state.as_mut().unwrap().seen_headers = true;
        Ok(())
    }

    pub fn send_trailers(&mut self, trailers: Headers) -> Result<(), SendError> {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()));
        }
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(trailers),
                last: true,
            },
        ))?;
        self.state = None;
        Ok(())
    }

    // TODO: explicit executor parameter
    pub fn pull_from_stream(&mut self, stream: HttpStreamAfterHeaders) -> Result<(), SendError> {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()));
        }

        match self.state.take() {
            Some(CanSendData {
                write_tx,
                out_window,
                ..
            }) => {
                // TODO: why client died
                write_tx
                    .unbounded_send(
                        CommonToWriteMessage::Pull(self.stream_id, stream, out_window).into(),
                    ).map_err(|e| SendError::ConnectionDied(Arc::new(e)))
            }
            None => Err(SendError::IncorrectState(SenderState::Done)),
        }
    }

    pub fn pull_bytes_from_stream<S>(&mut self, stream: S) -> Result<(), SendError>
    where
        S: Stream<Item = Bytes, Error = error::Error> + Send + 'static,
    {
        self.pull_from_stream(HttpStreamAfterHeaders::bytes(stream))
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> Result<(), SendError> {
        // TODO: do nothing if stream is explicitly closed
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnd(stream_id, error_code))?;
        self.state.take();
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), SendError> {
        self.reset(ErrorCode::NoError)
    }
}

impl<T: Types> Drop for CommonSender<T> {
    fn drop(&mut self) {
        // TODO: different message if panicked
        if self.state() != SenderState::Done {
            warn!("Sender was not properly finished, sending RST_STREAM InternalError");
        }
        drop(self.reset(ErrorCode::InternalError))
    }
}
