use common::conn_command_channel::ConnCommandSender;
use common::increase_in_window::IncreaseInWindow;
use common::stream_from_network::StreamFromNetwork;
use common::stream_queue_sync::stream_queue_sync;
use server::increase_in_window::ServerIncreaseInWindow;
use server::stream_handler::ServerStreamHandler;
use server::stream_handler::ServerStreamHandlerHolder;
use server::types::ServerTypes;
use Headers;
use HttpStreamAfterHeaders;
use StreamId;

pub struct ServerRequest<'a> {
    /// Request headers
    pub headers: Headers,
    /// True if requests ends with headers
    pub end_stream: bool,
    pub(crate) stream_id: StreamId,
    /// Stream in window size at the moment of request start
    pub(crate) in_window_size: u32,
    pub(crate) stream_handler: &'a mut Option<ServerStreamHandlerHolder>,
    pub(crate) to_write_tx: &'a ConnCommandSender<ServerTypes>,
}

impl<'a> ServerRequest<'a> {
    pub fn make_stream(&mut self) -> HttpStreamAfterHeaders {
        if self.end_stream {
            HttpStreamAfterHeaders::empty()
        } else {
            self.register_stream_handler(|increase_in_window, in_window_size| {
                let (inc_tx, inc_rx) = stream_queue_sync();
                let stream_from_network = StreamFromNetwork {
                    rx: inc_rx,
                    increase_in_window: increase_in_window.0,
                    in_window_size,
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
    pub fn register_stream_handler<F, H, R>(&mut self, f: F) -> R
    where
        F: FnOnce(ServerIncreaseInWindow, u32) -> (H, R),
        H: ServerStreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = ServerIncreaseInWindow(IncreaseInWindow {
            stream_id: self.stream_id,
            to_write_tx: self.to_write_tx.clone(),
        });
        let (h, r) = f(increase_window, self.in_window_size);
        *self.stream_handler = Some(ServerStreamHandlerHolder(Box::new(h)));
        r
    }
}
