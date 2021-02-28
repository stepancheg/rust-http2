#![allow(dead_code)]

use futures::stream::Stream;
use std::task::Poll;

use crate::solicit::DEFAULT_SETTINGS;

use super::stream_queue_sync::StreamQueueSyncReceiver;
use super::types::Types;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use futures::task::Context;
use std::convert::TryFrom;
use std::pin::Pin;

/// Stream that provides data from network.
/// Most importantly, it increases WINDOW.
pub(crate) struct StreamFromNetwork<T: Types> {
    pub rx: StreamQueueSyncReceiver<T>,
    pub increase_in_window: IncreaseInWindow<T>,
}

impl<T: Types> StreamFromNetwork<T> {
    /// Fetch the next message without increasing the window size.
    pub fn poll_next_no_auto(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrHeadersWithFlag>>> {
        let part = match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Ready(Some(Ok(part))) => part,
        };

        match &part.content {
            DataOrHeaders::Headers(..) => {}
            DataOrHeaders::Data(b) => {
                self.increase_in_window
                    .data_frame_received(u32::try_from(b.len()).unwrap());
            }
        };

        Poll::Ready(Some(Ok(part)))
    }
}

impl<T: Types> Stream for StreamFromNetwork<T> {
    type Item = crate::Result<DataOrHeadersWithFlag>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrHeadersWithFlag>>> {
        let me = self.get_mut();
        let part = match Pin::new(&mut *me).poll_next_no_auto(cx)? {
            Poll::Ready(Some(part)) => part,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        if let DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(_b),
            ..
        } = &part
        {
            // TODO: use different
            // TODO: increment after process of the frame (i. e. on next poll)
            let edge = DEFAULT_SETTINGS.initial_window_size / 2;
            if me.increase_in_window.in_window_size() < edge {
                let inc = DEFAULT_SETTINGS.initial_window_size;
                me.increase_in_window.increase_window(inc)?;
            }
        }

        Poll::Ready(Some(Ok(part)))
    }
}

impl<T: Types> Drop for StreamFromNetwork<T> {
    fn drop(&mut self) {
        // TODO: reset stream
    }
}
