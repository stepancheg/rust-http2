use std::fmt;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::Stream;

use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::deref_pin::DerefPinMut;
use crate::DataOrTrailers;
use crate::Headers;
use bytes::Bytes;

pub trait HttpStreamAfterHeaders2: fmt::Debug + Unpin + Send + 'static {
    /// Fetch the next message without increasing the window size.
    ///
    /// This is lower-level operation, might not be needed to be used.
    fn poll_next_no_auto(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>>;

    /// `Stream`-like operation.
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>>;

    /// Poll `DATA` frame from the stream.
    ///
    /// Note when this operation returns `None`, trailers can still be fetched
    /// with [`poll_trailers`].
    fn poll_data(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>>;

    /// Poll trailing `HEADERS` frame from the stream.
    ///
    /// This operation returns `None` if not all `DATA` frames fetched yet.
    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<Headers>>>;

    /// Current effective Window size known to the caller. This is a sum of:
    ///
    /// * window size known to the peer
    /// * all `DATA` messages traveling in the network
    /// * all `DATA` frame not yet fetched from this stream
    fn in_window_size(&self) -> u32;

    /// Explicitly request to increase Window size.
    ///
    /// Note this operation might be expensive, because each invocation sends a `WINDOW_UPDATE`
    /// frame. You probably might want to use default or update the default with
    /// [`set_auto_in_window_size`].
    fn inc_in_window(&mut self, delta: u32) -> crate::Result<()>;

    /// Window will be increased each time current window size drops below the half
    /// of given value when `poll_next` is used.
    fn set_auto_in_window_size(&mut self, window_size: u32) -> crate::Result<()>;

    fn into_stream(self) -> HttpStreamAfterHeadersAsStream<Self>
    where
        Self: Sized,
    {
        HttpStreamAfterHeadersAsStream(self)
    }

    fn into_stream_with_flag(self) -> HttpStreamAfterHeadersWithFlagAsStream<Self>
    where
        Self: Sized,
    {
        HttpStreamAfterHeadersWithFlagAsStream(self)
    }

    fn filter_data(self) -> HttpStreamAfterHeadersDataStream<Self>
    where
        Self: Sized,
    {
        HttpStreamAfterHeadersDataStream(self)
    }
}

pub struct HttpStreamAfterHeadersAsStream<S: HttpStreamAfterHeaders2>(S);

impl<S: HttpStreamAfterHeaders2> Stream for HttpStreamAfterHeadersAsStream<S> {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0).poll_next(cx)
    }
}

pub struct HttpStreamAfterHeadersWithFlagAsStream<S: HttpStreamAfterHeaders2>(S);

impl<S: HttpStreamAfterHeaders2> Stream for HttpStreamAfterHeadersWithFlagAsStream<S> {
    type Item = crate::Result<DataOrHeadersWithFlag>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0)
            .poll_next(cx)
            .map_ok(|s| s.into_part())
    }
}

pub struct HttpStreamAfterHeadersDataStream<S: HttpStreamAfterHeaders2>(S);

impl<S: HttpStreamAfterHeaders2> Stream for HttpStreamAfterHeadersDataStream<S> {
    type Item = crate::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0).poll_data(cx)
    }
}

pub type HttpStreamAfterHeadersBox = Pin<Box<dyn HttpStreamAfterHeaders2>>;

#[derive(Debug)]
pub(crate) struct HttpStreamAfterHeaders2Empty;

impl HttpStreamAfterHeaders2 for HttpStreamAfterHeaders2Empty {
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        Poll::Ready(None)
    }

    fn in_window_size(&self) -> u32 {
        0
    }

    fn inc_in_window(&mut self, _delta: u32) -> crate::Result<()> {
        Ok(())
    }

    fn set_auto_in_window_size(&mut self, _window_size: u32) -> crate::Result<()> {
        Ok(())
    }

    fn poll_next_no_auto(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        Poll::Ready(None)
    }

    fn poll_data(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<Bytes>>> {
        Poll::Ready(None)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<Headers>>> {
        Poll::Ready(None)
    }
}

impl HttpStreamAfterHeaders2 for Pin<Box<dyn HttpStreamAfterHeaders2>> {
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        self.deref_pin().poll_next(_cx)
    }

    fn in_window_size(&self) -> u32 {
        self.deref().in_window_size()
    }

    fn inc_in_window(&mut self, delta: u32) -> crate::Result<()> {
        self.deref_mut().inc_in_window(delta)
    }

    fn set_auto_in_window_size(&mut self, window_size: u32) -> crate::Result<()> {
        self.deref_mut().set_auto_in_window_size(window_size)
    }

    fn poll_next_no_auto(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        self.deref_pin().poll_next_no_auto(cx)
    }

    fn poll_data(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>> {
        self.deref_pin().poll_data(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<Headers>>> {
        self.deref_pin().poll_trailers(cx)
    }
}
