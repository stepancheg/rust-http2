use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::increase_in_window_common::IncreaseInWindowCommon;
use crate::common::stream_after_headers::HttpStreamAfterHeadersEmpty;
use crate::common::stream_after_headers::StreamAfterHeadersBox;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::death::channel::DeathAwareSender;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::server::conn::ServerToWriteMessage;
use crate::server::stream_handler::ServerRequestStreamHandler;
use crate::server::stream_handler::ServerRequestStreamHandlerHolder;
use crate::server::types::ServerTypes;
use crate::EndStream;
use crate::Headers;
use crate::StreamAfterHeaders;
use crate::StreamId;
use tokio::runtime::Handle;

/// A request object provided to the server callback.
pub struct ServerRequest<'a> {
    pub(crate) loop_handle: &'a Handle,
    /// Request headers.
    pub headers: Headers,
    /// True if requests ends with headers (no body).
    pub end_stream: EndStream,
    pub(crate) stream_id: StreamId,
    /// Stream in window size at the moment of request start
    pub(crate) in_window_size: u32,
    pub(crate) stream_handler: &'a mut Option<ServerRequestStreamHandlerHolder>,
    pub(crate) to_write_tx: &'a DeathAwareSender<ServerToWriteMessage, ConnDiedType>,
    pub(crate) conn_died: &'a SomethingDiedErrorHolder<ConnDiedType>,
}

impl<'a> ServerRequest<'a> {
    /// Get a loop handle.
    pub fn loop_handle(&self) -> Handle {
        self.loop_handle.clone()
    }

    /// Convert a request into a pullable stream.
    pub fn into_stream(self) -> impl StreamAfterHeaders {
        match self.end_stream {
            EndStream::Yes => Box::pin(HttpStreamAfterHeadersEmpty),
            EndStream::No => {
                let conn_died = self.conn_died.clone();
                self.register_stream_handler_internal::<_, _, StreamAfterHeadersBox>(
                    move |increase_in_window| {
                        let (inc_tx, inc_rx) = stream_queue_sync(conn_died);
                        let stream_from_network =
                            StreamFromNetwork::new(inc_rx, increase_in_window);
                        (inc_tx, Box::pin(stream_from_network))
                    },
                )
            }
        }
    }

    /// Register synchronous stream handler (callback will be called immediately
    /// when new data arrives). Note that increasing in window size is the handler
    /// responsibility.
    fn register_stream_handler_internal<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(IncreaseInWindowCommon<ServerTypes>) -> (H, R),
        H: ServerRequestStreamHandler,
    {
        assert!(self.stream_handler.is_none());
        let increase_window = IncreaseInWindowCommon {
            stream_id: self.stream_id,
            in_window_size: self.in_window_size,
            to_write_tx: self.to_write_tx.clone(),
        };
        let (h, r) = f(increase_window);
        *self.stream_handler = Some(ServerRequestStreamHandlerHolder(Box::new(h)));
        r
    }

    /// Register synchronous stream handler (callback will be called immediately
    /// when new data arrives). Note that increasing in window size is the handler
    /// responsibility.
    pub fn register_stream_handler<F, H, R>(self, f: F) -> R
    where
        F: FnOnce(IncreaseInWindow) -> (H, R),
        H: ServerRequestStreamHandler,
    {
        self.register_stream_handler_internal(|i| f(IncreaseInWindow::from(i)))
    }
}
