use crate::solicit::end_stream::EndStream;
use crate::solicit_async::TryStreamBox;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;
use futures::Stream;
use futures::TryStreamExt;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

/// Writer-part of the stream.
///
/// This is used for both client request and server response.
pub trait SinkAfterHeaders: Unpin + Send + 'static {
    /// Wait until the sink is ready to read data.
    ///
    /// Wait until both stream and connection out windows are greater than zero.
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>>;

    /// Send data to the stream.
    ///
    /// Note this operation succeeds even if out window is empty. In this case
    /// data will be buffered, and sent only when peer announces window increase.
    fn send_data_impl(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()>;

    /// Send data to the stream.
    fn send_data(&mut self, data: Bytes) -> crate::Result<()> {
        self.send_data_impl(data, EndStream::No)
    }

    /// Send data to the stream and close-local the stream.
    fn send_data_end_of_stream(&mut self, data: Bytes) -> crate::Result<()> {
        self.send_data_impl(data, EndStream::Yes)
    }

    /// Send trailers to the stream. The stream is closed-local after this operation.
    fn send_trailers(&mut self, trailers: Headers) -> crate::Result<()>;

    /// Reset the stream.
    fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()>;

    /// Close the stream. Empty `DATA` frame with `END_STREAM` flag will be sent.
    fn close(&mut self) -> crate::Result<()> {
        self.reset(ErrorCode::NoError)
    }

    /// Spawn a task which will pull the stream into this sink.
    fn pull_from_stream_dyn(&mut self, stream: TryStreamBox<DataOrTrailers>) -> crate::Result<()>;

    /// Spawn a task which will pull the stream into this sink.
    fn pull_from_stream<S: Stream<Item = crate::Result<DataOrTrailers>> + Send + 'static>(
        &mut self,
        stream: S,
    ) -> crate::Result<()>
    where
        Self: Sized,
    {
        self.pull_from_stream_dyn(Box::pin(stream))
    }

    /// Spawn a task which will pull the stream into this sink.
    fn pull_bytes_from_stream<S>(&mut self, stream: S) -> crate::Result<()>
    where
        S: Stream<Item = crate::Result<Bytes>> + Send + 'static,
        Self: Sized,
    {
        self.pull_from_stream(stream.map_ok(|b| DataOrTrailers::Data(b, EndStream::No)))
    }
}

fn _assert_unsized(s: &dyn SinkAfterHeaders) {
    let _ = s.clone();
}

/// Alias for `Box` of `dyn SinkAfterHeaders`.
pub type SinkAfterHeadersBox = Pin<Box<dyn SinkAfterHeaders>>;

impl SinkAfterHeaders for SinkAfterHeadersBox {
    fn send_data_impl(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()> {
        (&mut **self).send_data_impl(data, end_stream)
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        (&mut **self).poll(cx)
    }

    fn send_trailers(&mut self, trailers: Headers) -> crate::Result<()> {
        (&mut **self).send_trailers(trailers)
    }

    fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()> {
        (&mut **self).reset(error_code)
    }

    fn pull_from_stream_dyn(&mut self, stream: TryStreamBox<DataOrTrailers>) -> crate::Result<()>
    where
        Self: Sized,
    {
        (&mut **self).pull_from_stream_dyn(stream)
    }
}
