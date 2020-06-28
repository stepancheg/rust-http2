use std::panic;

use futures::future;
use futures::stream;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use std::task::Poll;

use bytes::Bytes;

use crate::{error, result};

use crate::solicit::header::Headers;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use crate::misc::any_to_string;
use crate::solicit::end_stream::EndStream;
use futures::stream::StreamExt;
use futures::task::Context;
use std::pin::Pin;

/// Stream frame content after initial headers
pub enum DataOrTrailers {
    /// DATA frame
    Data(Bytes, EndStream),
    /// HEADERS frame with END_STREAM flag
    Trailers(Headers),
}

impl DataOrTrailers {
    pub fn intermediate_data(data: Bytes) -> Self {
        DataOrTrailers::Data(data, EndStream::No)
    }

    pub fn into_part(self) -> DataOrHeadersWithFlag {
        match self {
            DataOrTrailers::Data(data, end_stream) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                last: end_stream == EndStream::Yes,
            },
            DataOrTrailers::Trailers(headers) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(headers),
                last: true,
            },
        }
    }
}

/// Stream of DATA or HEADERS frames after initial HEADERS.
///
/// Technically this class is just a simple wrapper over boxed `Stream`
/// of `DataOrTrailers` objects.
///
/// Most users won't need anything except data, so this type provides
/// convenient constructors and accessors.
pub struct HttpStreamAfterHeaders(
    pub Pin<Box<dyn Stream<Item = result::Result<DataOrTrailers>> + Send + 'static>>,
);

impl HttpStreamAfterHeaders {
    // constructors

    /// Box and wraper futures stream of `DataOrTrailers`.
    pub fn new<S>(s: S) -> HttpStreamAfterHeaders
    where
        S: Stream<Item = result::Result<DataOrTrailers>> + Send + 'static,
    {
        HttpStreamAfterHeaders(Box::pin(s))
    }

    pub(crate) fn from_parts<S>(s: S) -> HttpStreamAfterHeaders
    where
        S: Stream<Item = result::Result<DataOrHeadersWithFlag>> + Send + 'static,
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
        S: Stream<Item = result::Result<Bytes>> + Send + 'static,
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
    pub fn filter_data(self) -> impl Stream<Item = result::Result<Bytes>> + Send {
        self.try_filter_map(|p| {
            future::ok(match p {
                DataOrTrailers::Data(data, ..) => Some(data),
                DataOrTrailers::Trailers(..) => None,
            })
        })
    }

    pub(crate) fn into_flag_stream(
        self,
    ) -> impl Stream<Item = result::Result<DataOrHeadersWithFlag>> + Send {
        TryStreamExt::map_ok(self.0, |r| DataOrTrailers::into_part(r))
    }

    // TODO: drop
    pub(crate) fn _into_part_stream(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(self.into_flag_stream())
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
                    Err(error::Error::HandlerPanicked(e))
                }
            })
        }))
    }
}

impl Stream for HttpStreamAfterHeaders {
    type Item = result::Result<DataOrTrailers>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(context)
    }
}
