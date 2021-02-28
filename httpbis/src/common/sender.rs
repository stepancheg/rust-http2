use crate::common::conn_write::CommonToWriteMessage;
use crate::common::types::Types;
use crate::common::window_size::StreamOutWindowReceiver;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::death::channel::DeathAwareSender;
use crate::solicit::stream_id::StreamId;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;

use crate::common::sink_after_headers::SinkAfterHeaders;
use crate::death::error_holder::ConnDiedType;
use crate::solicit::end_stream::EndStream;
use crate::solicit_async::TryStreamBox;
use futures::task::Context;
use std::sync::Arc;
use std::task::Poll;

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum SenderState {
    ExpectingHeaders,
    ExpectingBodyOrTrailers,
    Done,
}

#[derive(Debug)]
pub enum SendError {
    // TODO: drop
    ConnectionDied(Arc<crate::Error>),
    IncorrectState(SenderState),
}

struct CanSendData<T: Types> {
    write_tx: DeathAwareSender<T::ToWriteMessage, ConnDiedType>,
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
        write_tx: DeathAwareSender<T::ToWriteMessage, ConnDiedType>,
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

    /// Create sender in "done" state (nothing to send)
    pub fn new_done(stream_id: StreamId) -> Self {
        CommonSender {
            state: None,
            stream_id,
        }
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

    pub fn send_headers(&mut self, headers: Headers) -> Result<(), SendError> {
        self.send_headers_impl(headers, EndStream::No)
    }

    pub fn send_headers_end_of_stream(&mut self, headers: Headers) -> Result<(), SendError> {
        self.send_headers_impl(headers, EndStream::Yes)
    }

    pub fn send_headers_impl(
        &mut self,
        headers: Headers,
        end_stream: EndStream,
    ) -> Result<(), SendError> {
        if self.state() != SenderState::ExpectingHeaders {
            return Err(SendError::IncorrectState(self.state()));
        }
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(headers),
                end_stream,
            },
        ))?;
        if end_stream == EndStream::Yes {
            self.state.take();
        } else {
            self.state.as_mut().unwrap().seen_headers = true;
        }
        Ok(())
    }
}

impl<T: Types> SinkAfterHeaders for CommonSender<T> {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        match self.state {
            Some(ref mut state) => state.out_window.poll(cx),
            // TODO: different error
            None => Poll::Ready(Ok(())),
        }
    }

    fn send_data_impl(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()> {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()).into());
        }
        let stream_id = self.stream_id;
        self.get_can_send()?.out_window.decrease(data.len());
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                end_stream,
            },
        ))?;
        if end_stream == EndStream::Yes {
            self.state.take();
        }
        Ok(())
    }

    fn send_trailers(&mut self, trailers: Headers) -> crate::Result<()> {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()).into());
        }
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(trailers),
                end_stream: EndStream::Yes,
            },
        ))?;
        self.state = None;
        Ok(())
    }

    fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()> {
        // TODO: do nothing if stream is explicitly closed
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnd(stream_id, error_code))?;
        self.state.take();
        Ok(())
    }

    fn pull_from_stream_dyn(&mut self, stream: TryStreamBox<DataOrTrailers>) -> crate::Result<()>
    where
        Self: Sized,
    {
        if self.state() != SenderState::ExpectingBodyOrTrailers {
            return Err(SendError::IncorrectState(self.state()).into());
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
                    )
                    .map_err(|e| SendError::ConnectionDied(Arc::new(e)).into())
            }
            None => Err(SendError::IncorrectState(SenderState::Done).into()),
        }
    }
}

impl<T: Types> Drop for CommonSender<T> {
    fn drop(&mut self) {
        // TODO: different message if panicked
        let state = self.state();
        if state != SenderState::Done {
            warn!(
                "sender was not properly finished, state {:?}, sending RST_STREAM InternalError",
                self.state()
            );
            drop(self.reset(ErrorCode::InternalError));
        }
    }
}
