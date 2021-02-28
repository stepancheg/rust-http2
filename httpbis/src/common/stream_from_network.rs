#![allow(dead_code)]

use std::mem;
use std::pin::Pin;
use std::task::Poll;

use bytes::Bytes;
use futures::stream::Stream;
use futures::task::Context;

use super::types::Types;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::stream_queue_sync::StreamQueueSyncReceiver;
use crate::solicit::end_stream::EndStream;
use crate::DataOrTrailers;
use crate::Headers;
use crate::HttpStreamAfterHeaders2;

#[derive(Debug)]
struct DataDontForgetToDecrease(Bytes);

impl DataDontForgetToDecrease {
    fn into_inner<T: Types>(self, increase_in_window: &mut IncreaseInWindow<T>) -> Bytes {
        increase_in_window.data_frame_received(self.0.len());
        self.0
    }
}

#[derive(Debug)]
enum DataOrTrailersDontForgetToDecrease {
    Data(DataDontForgetToDecrease, EndStream),
    Trailers(Headers),
}

impl DataOrTrailersDontForgetToDecrease {
    fn new(data_or_trailers: DataOrTrailers) -> DataOrTrailersDontForgetToDecrease {
        match data_or_trailers {
            DataOrTrailers::Data(bytes, end_stream) => DataOrTrailersDontForgetToDecrease::Data(
                DataDontForgetToDecrease(bytes),
                end_stream,
            ),
            DataOrTrailers::Trailers(trailers) => {
                DataOrTrailersDontForgetToDecrease::Trailers(trailers)
            }
        }
    }

    fn into_inner<T: Types>(self, increase_in_window: &mut IncreaseInWindow<T>) -> DataOrTrailers {
        match self {
            DataOrTrailersDontForgetToDecrease::Data(bytes, end_of_stream) => {
                DataOrTrailers::Data(bytes.into_inner(increase_in_window), end_of_stream)
            }
            DataOrTrailersDontForgetToDecrease::Trailers(trailers) => {
                DataOrTrailers::Trailers(trailers)
            }
        }
    }
}

/// Stream that provides data from network.
/// Most importantly, it increases WINDOW.
#[derive(Debug)]
pub(crate) struct StreamFromNetwork<T: Types> {
    rx: StreamQueueSyncReceiver<T>,
    increase_in_window: IncreaseInWindow<T>,
    auto_in_window_size: u32,
    next_frame: Option<DataOrTrailersDontForgetToDecrease>,
}

impl<T: Types> StreamFromNetwork<T> {
    pub fn new(rx: StreamQueueSyncReceiver<T>, increase_in_window: IncreaseInWindow<T>) -> Self {
        StreamFromNetwork {
            rx,
            auto_in_window_size: increase_in_window.in_window_size,
            increase_in_window,
            next_frame: None,
        }
    }
}

impl<T: Types> StreamFromNetwork<T> {
    fn poll_prefetch(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        if self.next_frame.is_some() {
            return Poll::Ready(Ok(()));
        }

        let part = match Pin::new(&mut self.rx).poll_next(cx)? {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(Ok(())),
            Poll::Ready(Some(part)) => part,
        };

        self.next_frame = Some(DataOrTrailersDontForgetToDecrease::new(part));

        Poll::Ready(Ok(()))
    }

    fn auto_update_in_window_size(&mut self) -> crate::Result<()> {
        // TODO: use update default window size
        let edge = self.auto_in_window_size / 2;
        if self.increase_in_window.in_window_size() < edge {
            let inc = self.auto_in_window_size;
            self.increase_in_window.increase_window(inc)?;
        }
        Ok(())
    }
}

impl<T: Types> HttpStreamAfterHeaders2 for StreamFromNetwork<T> {
    fn in_window_size(&self) -> u32 {
        self.increase_in_window.in_window_size
    }

    fn inc_in_window(&mut self, inc: u32) -> crate::Result<()> {
        self.increase_in_window.increase_window(inc)
    }

    fn set_auto_in_window_size(&mut self, window_size: u32) -> crate::Result<()> {
        self.auto_in_window_size = window_size;
        Ok(())
    }

    /// Fetch the next message without increasing the window size.
    fn poll_next_no_auto(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        match self.poll_prefetch(cx)? {
            Poll::Ready(()) => {}
            Poll::Pending => return Poll::Pending,
        }

        match self.next_frame.take() {
            None => Poll::Ready(None),
            Some(f) => Poll::Ready(Some(Ok(f.into_inner(&mut self.increase_in_window)))),
        }
    }

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        let me = self.get_mut();
        let part = match Pin::new(&mut *me).poll_next_no_auto(cx)? {
            Poll::Ready(Some(part)) => part,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        match &part {
            DataOrTrailers::Data(..) => {
                // TODO: consider incrementing after processing of the frame (i. e. on next poll)
                me.auto_update_in_window_size()?;
            }
            DataOrTrailers::Trailers(..) => {}
        }

        Poll::Ready(Some(Ok(part)))
    }

    fn poll_data(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>> {
        let me = self.get_mut();
        match me.poll_prefetch(cx)? {
            Poll::Ready(()) => {}
            Poll::Pending => return Poll::Pending,
        }
        match me.next_frame {
            Some(DataOrTrailersDontForgetToDecrease::Data(..)) => {
                match mem::take(&mut me.next_frame) {
                    Some(DataOrTrailersDontForgetToDecrease::Data(bytes, ..)) => {
                        let bytes = bytes.into_inner(&mut me.increase_in_window);
                        me.auto_update_in_window_size()?;
                        Poll::Ready(Some(Ok(bytes)))
                    }
                    _ => unreachable!(),
                }
            }
            _ => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<Headers>>> {
        let me = self.get_mut();
        match me.poll_prefetch(cx)? {
            Poll::Ready(()) => {}
            Poll::Pending => return Poll::Pending,
        }
        match me.next_frame {
            Some(DataOrTrailersDontForgetToDecrease::Trailers(..)) => {
                match mem::take(&mut me.next_frame) {
                    Some(DataOrTrailersDontForgetToDecrease::Trailers(trailers)) => {
                        Poll::Ready(Some(Ok(trailers)))
                    }
                    _ => unreachable!(),
                }
            }
            _ => Poll::Ready(None),
        }
    }
}

impl<T: Types> Drop for StreamFromNetwork<T> {
    fn drop(&mut self) {
        // TODO: reset stream
    }
}
