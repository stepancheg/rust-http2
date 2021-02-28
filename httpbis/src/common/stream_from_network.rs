#![allow(dead_code)]

use futures::stream::Stream;
use std::task::Poll;

use crate::solicit::DEFAULT_SETTINGS;

use super::stream_queue_sync::StreamQueueSyncReceiver;
use super::types::Types;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::DataOrTrailers;
use crate::Headers;
use crate::HttpStreamAfterHeaders2;
use bytes::Bytes;
use futures::task::Context;
use std::convert::TryFrom;
use std::mem;
use std::pin::Pin;

/// Stream that provides data from network.
/// Most importantly, it increases WINDOW.
#[derive(Debug)]
pub(crate) struct StreamFromNetwork<T: Types> {
    rx: StreamQueueSyncReceiver<T>,
    increase_in_window: IncreaseInWindow<T>,
    auto_in_window_size: u32,
    next_frame: Option<DataOrTrailers>,
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

        let part = match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e)),
            Poll::Ready(None) => return Poll::Ready(Ok(())),
            Poll::Ready(Some(Ok(part))) => part,
        };

        match &part {
            DataOrTrailers::Data(b, _) => {
                // TODO: increase only when returned
                self.increase_in_window
                    .data_frame_received(u32::try_from(b.len()).unwrap());
            }
            DataOrTrailers::Trailers(_) => {}
        }

        self.next_frame = Some(part);

        Poll::Ready(Ok(()))
    }

    fn auto_update_in_window_size(&mut self) -> crate::Result<()> {
        let edge = DEFAULT_SETTINGS.initial_window_size / 2;
        if self.increase_in_window.in_window_size() < edge {
            let inc = DEFAULT_SETTINGS.initial_window_size;
            self.increase_in_window.increase_window(inc)?;
        }
        Ok(())
    }
}

impl<T: Types> HttpStreamAfterHeaders2 for StreamFromNetwork<T> {
    fn in_window_size(&self) -> u32 {
        self.increase_in_window.in_window_size
    }

    fn increase_window(&mut self, inc: u32) -> crate::Result<()> {
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
            Some(f) => Poll::Ready(Some(Ok(f))),
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
                // TODO: use different
                // TODO: increment after process of the frame (i. e. on next poll)
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
            Some(DataOrTrailers::Data(..)) => match mem::take(&mut me.next_frame) {
                Some(DataOrTrailers::Data(bytes, ..)) => {
                    me.auto_update_in_window_size()?;
                    // TODO: update window size
                    Poll::Ready(Some(Ok(bytes)))
                }
                _ => unreachable!(),
            },
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
            Some(DataOrTrailers::Trailers(..)) => match mem::take(&mut me.next_frame) {
                Some(DataOrTrailers::Trailers(trailers)) => Poll::Ready(Some(Ok(trailers))),
                _ => unreachable!(),
            },
            _ => Poll::Ready(None),
        }
    }
}

impl<T: Types> Drop for StreamFromNetwork<T> {
    fn drop(&mut self) {
        // TODO: reset stream
    }
}
