use std::fmt;
use std::panic;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures::future;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::debug_undebug::DebugUndebug;
use crate::for_test::solicit::end_stream::EndStream;
use crate::misc::any_to_string;
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

    pub(crate) fn from_parts<S>(s: S) -> HttpStreamAfterHeaders
    where
        S: Stream<Item = crate::Result<DataOrHeadersWithFlag>> + Send + 'static,
    {
        HttpStreamAfterHeaders::new(s.map_ok(DataOrHeadersWithFlag::into_after_headers))
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

    pub fn once(part: DataOrHeaders) -> HttpStreamAfterHeaders {
        let part = match part {
            DataOrHeaders::Data(data) => DataOrTrailers::Data(data, EndStream::Yes),
            DataOrHeaders::Headers(header) => DataOrTrailers::Trailers(header),
        };
        HttpStreamAfterHeaders::new(stream::once(future::ok(part)))
    }

    /// Create simple response stream from bytes.
    ///
    /// Useful when bytes are available and length is small.
    ///
    /// If bytes object is large, it will be split into multiple frames.
    pub fn once_bytes<B>(bytes: B) -> HttpStreamAfterHeaders
    where
        B: Into<Bytes>,
    {
        HttpStreamAfterHeaders::once(DataOrHeaders::Data(bytes.into()))
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

    pub(crate) fn into_flag_stream(
        self,
    ) -> impl Stream<Item = crate::Result<DataOrHeadersWithFlag>> + Send {
        TryStreamExt::map_ok(self.0, |r| DataOrTrailers::into_part(r))
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> HttpStreamAfterHeaders {
        HttpStreamAfterHeaders::new(panic::AssertUnwindSafe(self.0).catch_unwind().then(|r| {
            future::ready(match r {
                Ok(r) => r,
                Err(e) => {
                    let e = any_to_string(e);
                    // TODO: send plain text error if headers weren't sent yet
                    warn!("handler panicked: {}", e);
                    Err(crate::Error::HandlerPanicked(e))
                }
            })
        }))
    }
}

impl Stream for HttpStreamAfterHeaders {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(context)
    }
}
