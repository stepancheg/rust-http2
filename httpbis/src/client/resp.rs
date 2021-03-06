use std::future::Future;

use crate::client::conn::ClientToWriteMessage;
use crate::client::handler::ClientResponseStreamHandler;
use crate::client::handler::ClientResponseStreamHandlerHolder;
use crate::client::resp_future::ClientResponseFutureImpl;
use crate::client::types::ClientTypes;
use crate::common::increase_in_window_common::IncreaseInWindowCommon;
use crate::death::channel::DeathAwareSender;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::Headers;
use crate::IncreaseInWindow;
use crate::StreamAfterHeadersBox;
use crate::StreamId;

pub struct ClientResponse<'a> {
    pub(crate) stream_handler: &'a mut Option<ClientResponseStreamHandlerHolder>,
    pub(crate) in_window_size: u32,
    pub(crate) stream_id: StreamId,
    pub(crate) to_write_tx: &'a DeathAwareSender<ClientToWriteMessage, ConnDiedType>,
    pub(crate) conn_died: &'a SomethingDiedErrorHolder<ConnDiedType>,
}

impl<'a> ClientResponse<'a> {
    pub fn into_stream(
        self,
    ) -> impl Future<Output = crate::Result<(Headers, StreamAfterHeadersBox)>> {
        let conn_died = self.conn_died.clone();
        self.register_stream_handler_internal(move |increase_in_window| {
            let (a, b) = ClientResponseFutureImpl::new(increase_in_window, conn_died);
            (a, b)
        })
    }

    fn register_stream_handler_internal<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(IncreaseInWindowCommon<ClientTypes>) -> (H, R),
        H: ClientResponseStreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = IncreaseInWindowCommon {
            stream_id: self.stream_id,
            in_window_size: self.in_window_size,
            to_write_tx: self.to_write_tx.clone(),
        };
        let (h, r) = f(increase_window);
        *self.stream_handler = Some(ClientResponseStreamHandlerHolder(Box::new(h)));
        r
    }

    /// Register synchronous stream handler (callback will be called immediately
    /// when new data arrives). Note that increasing in window size is the handler
    /// responsibility.
    pub fn register_stream_handler<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(IncreaseInWindow) -> (H, R),
        H: ClientResponseStreamHandler,
    {
        self.register_stream_handler_internal(|i| f(IncreaseInWindow::from(i)))
    }
}
