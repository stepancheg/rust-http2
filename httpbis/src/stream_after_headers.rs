use std::fmt;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures::future;
use futures::stream;
use futures::Stream;
use futures::TryStreamExt;

use crate::debug_undebug::DebugUndebug;
use crate::DataOrTrailers;

/// Stream of `DATA` or `HEADERS` frames after initial `HEADERS`.
///
/// Technically this class is just a simple wrapper over boxed `Stream`
/// of `DataOrTrailers` objects.
///
/// Most users won't need anything except data, so this type provides
/// convenient constructors and accessors.
pub struct HttpStreamAfterHeaders(
    pub Pin<Box<dyn Stream<Item = crate::Result<DataOrTrailers>> + Send + 'static>>,
);

impl fmt::Debug for HttpStreamAfterHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&DebugUndebug(self), f)
    }
}

impl HttpStreamAfterHeaders {
    // constructors

    /// Box and wraper futures stream of `DataOrTrailers`.
    pub fn new<S>(s: S) -> HttpStreamAfterHeaders
    where
        S: Stream<Item = crate::Result<DataOrTrailers>> + Send + 'static,
    {
        HttpStreamAfterHeaders(Box::pin(s))
    }

    /// Create an empty response stream (no body, no trailers).
    pub fn empty() -> HttpStreamAfterHeaders {
        HttpStreamAfterHeaders::new(stream::empty())
    }

    /// Create a response from a stream of bytes.
    pub fn bytes<S>(bytes: S) -> HttpStreamAfterHeaders
    where
        S: Stream<Item = crate::Result<Bytes>> + Send + 'static,
    {
        HttpStreamAfterHeaders::new(bytes.map_ok(DataOrTrailers::intermediate_data))
    }

    // getters

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> impl Stream<Item = crate::Result<Bytes>> + Send {
        self.try_filter_map(|p| {
            future::ok(match p {
                DataOrTrailers::Data(data, ..) => Some(data),
                DataOrTrailers::Trailers(..) => None,
            })
        })
    }
}

impl Stream for HttpStreamAfterHeaders {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(context)
    }
}
