use crate::common::increase_in_window::IncreaseInWindow;
use crate::common::stream_after_headers::HttpStreamAfterHeadersEmpty;
use crate::common::stream_after_headers::StreamAfterHeadersBox;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::death::channel::DeathAwareSender;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::server::conn::ServerToWriteMessage;
use crate::server::increase_in_window::ServerIncreaseInWindow;
use crate::server::stream_handler::ServerRequestStreamHandler;
use crate::server::stream_handler::ServerRequestStreamHandlerHolder;
use crate::EndStream;
use crate::Headers;
use crate::StreamAfterHeaders;
use crate::StreamId;

/// A request object provided to the server callback.
pub struct ServerRequest<'a> {
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
    /// Convert a request into a pullable stream.
    pub fn into_stream(self) -> impl StreamAfterHeaders {
        match self.end_stream {
            EndStream::Yes => Box::pin(HttpStreamAfterHeadersEmpty),
            EndStream::No => {
                let conn_died = self.conn_died.clone();
                self.register_stream_handler::<_, _, StreamAfterHeadersBox>(
                    move |increase_in_window| {
                        let (inc_tx, inc_rx) = stream_queue_sync(conn_died);
                        let stream_from_network =
                            StreamFromNetwork::new(inc_rx, increase_in_window.0);
                        (inc_tx, Box::pin(stream_from_network))
                    },
                )
            }
        }
    }

    /// Register synchronous stream handler (callback will be called immediately
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
