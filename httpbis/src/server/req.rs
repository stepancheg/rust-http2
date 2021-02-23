use crate::client_died_error_holder::ConnDiedType;
use crate::common::death_aware_channel::DeathAwareSender;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::server::conn::ServerToWriteMessage;
use crate::server::increase_in_window::ServerIncreaseInWindow;
use crate::server::stream_handler::ServerRequestStreamHandler;
use crate::server::stream_handler::ServerRequestStreamHandlerHolder;
use crate::Headers;
use crate::HttpStreamAfterHeaders;
use crate::StreamId;

pub struct ServerRequest<'a> {
    /// Request headers
    pub headers: Headers,
    /// True if requests ends with headers
    pub end_stream: bool,
    pub(crate) stream_id: StreamId,
    /// Stream in window size at the moment of request start
    pub(crate) in_window_size: u32,
    pub(crate) stream_handler: &'a mut Option<ServerRequestStreamHandlerHolder>,
    pub(crate) to_write_tx: &'a DeathAwareSender<ServerToWriteMessage, ConnDiedType>,
}

impl<'a> ServerRequest<'a> {
    pub fn make_stream(self) -> HttpStreamAfterHeaders {
        if self.end_stream {
            HttpStreamAfterHeaders::empty()
        } else {
            self.register_stream_handler(|increase_in_window| {
                let (inc_tx, inc_rx) = stream_queue_sync();
                let stream_from_network = StreamFromNetwork {
                    rx: inc_rx,
                    increase_in_window: increase_in_window.0,
                };

                (
                    inc_tx,
                    HttpStreamAfterHeaders::from_parts(stream_from_network),
                )
            })
        }
    }

    /// Register synchnous stream handler (callback will be called immediately
    /// when new data arrives). Note that increasing in window size is the handler
    /// responsibility.
    pub fn register_stream_handler<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(ServerIncreaseInWindow) -> (H, R),
        H: ServerRequestStreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = ServerIncreaseInWindow(IncreaseInWindow {
            stream_id: self.stream_id,
            in_window_size: self.in_window_size,
            to_write_tx: self.to_write_tx.clone(),
        });
        let (h, r) = f(increase_window);
        *self.stream_handler = Some(ServerRequestStreamHandlerHolder(Box::new(h)));
        r
    }
}
