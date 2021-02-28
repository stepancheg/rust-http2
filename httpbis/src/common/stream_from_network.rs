#![allow(dead_code)]

use futures::stream::Stream;
use std::task::Poll;

use crate::solicit::DEFAULT_SETTINGS;

use super::stream_queue_sync::StreamQueueSyncReceiver;
use super::types::Types;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::DataOrTrailers;
use crate::HttpStreamAfterHeaders2;
use futures::task::Context;
use std::convert::TryFrom;
use std::pin::Pin;

/// Stream that provides data from network.
/// Most importantly, it increases WINDOW.
pub(crate) struct StreamFromNetwork<T: Types> {
    rx: StreamQueueSyncReceiver<T>,
    increase_in_window: IncreaseInWindow<T>,
    auto_in_window_size: u32,
}

impl<T: Types> StreamFromNetwork<T> {
    pub fn new(rx: StreamQueueSyncReceiver<T>, increase_in_window: IncreaseInWindow<T>) -> Self {
        StreamFromNetwork {
            rx,
            auto_in_window_size: increase_in_window.in_window_size,
            increase_in_window,
        }
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
        let part = match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Ready(Some(Ok(part))) => part,
        };

        match &part {
            DataOrTrailers::Data(b, _) => {
                self.increase_in_window
                    .data_frame_received(u32::try_from(b.len()).unwrap());
            }
            DataOrTrailers::Trailers(_) => {}
        }

        Poll::Ready(Some(Ok(part)))
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
                let edge = DEFAULT_SETTINGS.initial_window_size / 2;
                if me.increase_in_window.in_window_size() < edge {
                    let inc = DEFAULT_SETTINGS.initial_window_size;
                    me.increase_in_window.increase_window(inc)?;
                }
            }
            DataOrTrailers::Trailers(..) => {}
        }

        Poll::Ready(Some(Ok(part)))
    }
}

impl<T: Types> Drop for StreamFromNetwork<T> {
    fn drop(&mut self) {
        // TODO: reset stream
    }
}
