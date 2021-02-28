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

pub trait SinkAfterHeaders: Unpin + Send + 'static {
    fn send_data_impl(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()>;

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>>;

    fn send_data(&mut self, data: Bytes) -> crate::Result<()> {
        self.send_data_impl(data, EndStream::No)
    }

    fn send_data_end_of_stream(&mut self, data: Bytes) -> crate::Result<()> {
        self.send_data_impl(data, EndStream::Yes)
    }

    fn send_trailers(&mut self, trailers: Headers) -> crate::Result<()>;

    fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()>;

    fn close(&mut self) -> crate::Result<()> {
        self.reset(ErrorCode::NoError)
    }

    fn pull_from_stream_dyn(&mut self, stream: TryStreamBox<DataOrTrailers>) -> crate::Result<()>;

    fn pull_from_stream<S: Stream<Item = crate::Result<DataOrTrailers>> + Send + 'static>(
        &mut self,
        stream: S,
    ) -> crate::Result<()>
    where
        Self: Sized,
    {
        self.pull_from_stream_dyn(Box::pin(stream))
    }

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
