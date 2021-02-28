use std::fmt;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::Stream;

use crate::deref_pin::DerefPinMut;
use crate::DataOrTrailers;

pub trait HttpStreamAfterHeaders2: fmt::Debug + Unpin + Send + 'static {
    /// `Stream`-like operation.
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>>;

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
    fn increase_window(&mut self, delta: u32) -> crate::Result<()>;

    /// Window will be increased each time current window size drops below the half
    /// of given value when `poll_next` is used.
    fn set_auto_in_window_size(&mut self, window_size: u32) -> crate::Result<()>;

    /// Fetch the next message without increasing the window size.
    ///
    /// This is lower-level operation, might not be needed to be used.
    fn poll_next_no_auto(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>>;

    fn into_stream(self) -> HttpStreamAfterHeadersAsStream<Self>
    where
        Self: Sized,
    {
        HttpStreamAfterHeadersAsStream(self)
    }
}

pub struct HttpStreamAfterHeadersAsStream<S: HttpStreamAfterHeaders2>(S);

impl<S: HttpStreamAfterHeaders2> Stream for HttpStreamAfterHeadersAsStream<S> {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0).poll_next(cx)
    }
}

pub type HttpStreamAfterHeader2Box = Pin<Box<dyn HttpStreamAfterHeaders2>>;

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

    fn increase_window(&mut self, _delta: u32) -> crate::Result<()> {
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

    fn increase_window(&mut self, delta: u32) -> crate::Result<()> {
        self.deref_mut().increase_window(delta)
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
}
