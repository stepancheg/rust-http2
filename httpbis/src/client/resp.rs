use crate::client::conn::ClientToWriteMessage;
use crate::client::handler::ClientResponseStreamHandler;
use crate::client::handler::ClientResponseStreamHandlerHolder;
use crate::client::increase_in_window::ClientIncreaseInWindow;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::death::channel::DeathAwareSender;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::ClientResponseFuture;
use crate::StreamId;

pub struct ClientResponse<'a> {
    pub(crate) stream_handler: &'a mut Option<ClientResponseStreamHandlerHolder>,
    pub(crate) in_window_size: u32,
    pub(crate) stream_id: StreamId,
    pub(crate) to_write_tx: &'a DeathAwareSender<ClientToWriteMessage, ConnDiedType>,
    pub(crate) conn_died: &'a SomethingDiedErrorHolder<ConnDiedType>,
}

impl<'a> ClientResponse<'a> {
    pub fn into_stream(self) -> ClientResponseFuture {
        let conn_died = self.conn_died.clone();
        self.register_stream_handler(move |increase_in_window| {
            let (inc_tx, inc_rx) = stream_queue_sync(conn_died);
            let stream_from_network = StreamFromNetwork {
                rx: inc_rx,
                increase_in_window: increase_in_window.0,
            };

            (
                inc_tx,
                ClientResponseFuture::from_stream(stream_from_network),
            )
        })
    }

    /// Register synchronous stream handler (callback will be called immediately
    /// when new data arrives). Note that increasing in window size is the handler
    /// responsibility.
    pub fn register_stream_handler<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(ClientIncreaseInWindow) -> (H, R),
        H: ClientResponseStreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = ClientIncreaseInWindow(IncreaseInWindow {
            stream_id: self.stream_id,
            in_window_size: self.in_window_size,
            to_write_tx: self.to_write_tx.clone(),
        });
        let (h, r) = f(increase_window);
        *self.stream_handler = Some(ClientResponseStreamHandlerHolder(Box::new(h)));
        r
    }
}
