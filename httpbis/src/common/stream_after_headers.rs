use std::cmp;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use futures::future;
use futures::Future;
use futures::Stream;
use tokio::io::AsyncRead;
use tokio::io::ReadBuf;

use crate::solicit_async::TryFutureBox;
use crate::solicit_async::TryStreamBox;
use crate::DataOrTrailers;
use crate::Headers;

pub trait StreamAfterHeaders: fmt::Debug + Unpin + Send + 'static {
    /// Fetch the next message without increasing the window size.
    ///
    /// This is lower-level operation, might not be needed to be used.
    fn poll_next_no_auto(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>>;

    /// Fetch next frame and auto-update in window.
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<DataOrTrailers>>>;

    /// Poll `DATA` frame from the stream.
    ///
    /// Note when this operation returns `None`, trailers can still be fetched
    /// with [`poll_trailers`](Self::poll_trailers).
    fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>>;

    /// Poll trailing `HEADERS` frame from the stream.
    ///
    /// This operation returns `None` if not all `DATA` frames fetched yet.
    fn poll_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Headers>>>;

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
    /// [`set_auto_in_window_size`](Self::set_auto_in_window_size).
    fn inc_in_window(&mut self, delta: u32) -> crate::Result<()>;

    /// Window will be increased each time current window size drops below the half
    /// of given value when `poll_next` is used.
    fn set_auto_in_window_size(&mut self, window_size: u32) -> crate::Result<()>;

    // Utilities

    /// Fetch next frame.
    fn next<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Option<DataOrTrailers>>> + 'a>> {
        Box::pin(future::poll_fn(move |cx| self.poll_next(cx)?.map(Ok)))
    }

    /// Fetch next `DATA`.
    fn next_data<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Option<Bytes>>> + Send + 'a>> {
        Box::pin(future::poll_fn(move |cx| self.poll_data(cx)?.map(Ok)))
    }

    /// Fetch trailers. This returns `None` if not all `DATA` frames processed.
    fn next_trailers<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Option<Headers>>> + 'a>> {
        Box::pin(future::poll_fn(move |cx| self.poll_trailers(cx)?.map(Ok)))
    }

    fn into_read(self) -> Pin<Box<dyn AsyncRead + Send + 'static>>
    where
        Self: Sized,
    {
        Box::pin(AsRead {
            stream: self,
            rem: Bytes::new(),
        })
    }

    fn into_stream(self) -> TryStreamBox<DataOrTrailers>
    where
        Self: Sized,
    {
        Box::pin(AsStream(self))
    }

    fn filter_data(self) -> TryStreamBox<Bytes>
    where
        Self: Sized,
    {
        Box::pin(DataStream(self))
    }

    fn collect_data(mut self) -> TryFutureBox<Bytes>
    where
        Self: Sized,
    {
        Box::pin(async move {
            let mut r = BytesMut::new();
            loop {
                match self.next_data().await? {
                    None => return Ok(r.freeze()),
                    Some(b) => {
                        // TODO: figure out how to efficiently extend from Bytes
                        r.put_slice(&b)
                    }
                }
            }
        })
    }
}

struct AsStream<S: StreamAfterHeaders>(S);

impl<S: StreamAfterHeaders> Stream for AsStream<S> {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0).poll_next(cx)
    }
}

struct DataStream<S: StreamAfterHeaders>(S);

impl<S: StreamAfterHeaders> Stream for DataStream<S> {
    type Item = crate::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().0).poll_data(cx)
    }
}

pub struct AsRead<S: StreamAfterHeaders> {
    stream: S,
    rem: Bytes,
}

impl<S: StreamAfterHeaders> AsyncRead for AsRead<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        // TODO: untested
        let me = self.get_mut();

        loop {
            if !me.rem.is_empty() {
                let min = cmp::min(me.rem.len(), buf.remaining());
                buf.put_slice(&me.rem.split_to(min));
                return Poll::Ready(Ok(()));
            }

            me.rem = match Pin::new(&mut me.stream).poll_data(cx) {
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e.into())),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Ok(bytes))) => bytes,
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

pub type StreamAfterHeadersBox = Pin<Box<dyn StreamAfterHeaders>>;

#[derive(Debug)]
pub(crate) struct HttpStreamAfterHeadersEmpty;

impl StreamAfterHeaders for HttpStreamAfterHeadersEmpty {
    fn poll_next(&mut self, _cx: &mut Context<'_>) -> Poll<Option<crate::Result<DataOrTrailers>>> {
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
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        Poll::Ready(None)
    }

    fn poll_data(&mut self, _cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>> {
        Poll::Ready(None)
    }

    fn poll_trailers(&mut self, _cx: &mut Context<'_>) -> Poll<Option<crate::Result<Headers>>> {
        Poll::Ready(None)
    }
}

impl StreamAfterHeaders for Pin<Box<dyn StreamAfterHeaders>> {
    fn poll_next(&mut self, _cx: &mut Context<'_>) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        self.deref_mut().poll_next(_cx)
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
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        self.deref_mut().poll_next_no_auto(cx)
    }

    fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>> {
        self.deref_mut().poll_data(cx)
    }

    fn poll_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Headers>>> {
        self.deref_mut().poll_trailers(cx)
    }
}
