use crate::client::increase_in_window::ClientIncreaseInWindow;
use crate::client::stream_handler::ClientResponseStreamHandler;
use crate::client::stream_handler::ClientResponseStreamHandlerHolder;
use crate::client::types::ClientTypes;
use crate::common::conn_command_channel::ConnCommandSender;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::Response;
use crate::StreamId;

pub struct ClientResponse<'a> {
    pub(crate) stream_handler: &'a mut Option<ClientResponseStreamHandlerHolder>,
    pub(crate) in_window_size: u32,
    pub(crate) stream_id: StreamId,
    pub(crate) to_write_tx: &'a ConnCommandSender<ClientTypes>,
}

impl<'a> ClientResponse<'a> {
    pub fn make_stream(self) -> Response {
        self.register_stream_handler(|increase_in_window| {
            let (inc_tx, inc_rx) = stream_queue_sync();
            let stream_from_network = StreamFromNetwork {
                rx: inc_rx,
                increase_in_window: increase_in_window.0,
            };

            (inc_tx, Response::from_stream(stream_from_network))
        })
    }

    /// Register synchnous stream handler (callback will be called immediately
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
