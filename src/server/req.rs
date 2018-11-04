use client_died_error_holder::ClientConnDiedType;
use client_died_error_holder::ClientDiedErrorHolder;
use common::increase_in_window::IncreaseInWindow;
use common::stream_from_network::StreamFromNetwork;
use common::stream_handler::StreamHandler;
use common::stream_queue_sync::stream_queue_sync;
use futures::sync::mpsc::UnboundedSender;
use server::conn::ServerToWriteMessage;
use server::increase_in_window::ServerIncreaseInWindow;
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
    pub(crate) stream_handler: &'a mut Option<Box<StreamHandler>>,
    pub(crate) conn_died_error_holder: &'a ClientDiedErrorHolder<ClientConnDiedType>,
    pub(crate) to_write_tx: &'a UnboundedSender<ServerToWriteMessage>,
}

impl<'a> ServerRequest<'a> {
    pub fn make_stream(&mut self) -> HttpStreamAfterHeaders {
        if self.end_stream {
            HttpStreamAfterHeaders::empty()
        } else {
            let client_died_error_holder = self.conn_died_error_holder.clone();
            let in_window_size = self.in_window_size;
            self.register_stream_handler(|increase_in_window| {
                let (inc_tx, inc_rx) = stream_queue_sync(client_died_error_holder);
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

    pub fn register_stream_handler<F, H, R>(&mut self, f: F) -> R
    where
        F: FnOnce(ServerIncreaseInWindow) -> (H, R),
        H: StreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = ServerIncreaseInWindow(IncreaseInWindow {
            stream_id: self.stream_id,
            to_write_tx: self.to_write_tx.clone(),
        });
        let (h, r) = f(increase_window);
        *self.stream_handler = Some(Box::new(h));
        r
    }
}
