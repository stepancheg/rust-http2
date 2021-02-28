use futures::stream;
use futures::stream::Stream;

use bytes::Bytes;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_trailers::DataOrTrailers;
use crate::misc::any_to_string;
use crate::solicit::end_stream::EndStream;
use crate::solicit::header::Headers;
use crate::solicit_async::TryStreamBox;
use futures::future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::task::Context;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::task::Poll;

/// Stream frame content with END_STREAM flag
#[derive(Debug)]
pub struct DataOrHeadersWithFlag {
    pub content: DataOrHeaders,
    pub end_stream: EndStream,
}

impl DataOrHeadersWithFlag {
    pub fn last_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            end_stream: EndStream::Yes,
        }
    }

    pub fn intermediate_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            end_stream: EndStream::No,
        }
    }

    pub fn intermediate_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            end_stream: EndStream::No,
        }
    }

    pub fn last_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            end_stream: EndStream::Yes,
        }
    }

    pub fn into_after_headers(self) -> DataOrTrailers {
        let DataOrHeadersWithFlag {
            content,
            end_stream,
        } = self;
        match content {
            DataOrHeaders::Data(data) => DataOrTrailers::Data(data, end_stream),
            DataOrHeaders::Headers(headers) => DataOrTrailers::Trailers(headers),
        }
    }
}

impl From<DataOrTrailers> for DataOrHeadersWithFlag {
    fn from(d: DataOrTrailers) -> Self {
        match d {
            DataOrTrailers::Data(data, end_stream) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                end_stream,
            },
            DataOrTrailers::Trailers(trailers) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(trailers),
                end_stream: EndStream::Yes,
            },
        }
    }
}

/// Stream of DATA of HEADER frames
pub struct DataOrHeadersWithFlagStream(pub TryStreamBox<DataOrHeadersWithFlag>);

impl DataOrHeadersWithFlagStream {
    // constructors

    pub fn new<S>(s: S) -> DataOrHeadersWithFlagStream
    where
        S: Stream<Item = crate::Result<DataOrHeadersWithFlag>> + Send + 'static,
    {
        DataOrHeadersWithFlagStream(Box::pin(s))
    }

    pub fn empty() -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(stream::empty())
    }

    pub fn bytes<S>(bytes: S) -> DataOrHeadersWithFlagStream
    where
        S: Stream<Item = crate::Result<Bytes>> + Send + 'static,
    {
        DataOrHeadersWithFlagStream::new(bytes.map_ok(DataOrHeadersWithFlag::intermediate_data))
    }

    pub fn once(part: DataOrHeaders) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(stream::once(future::ok(DataOrHeadersWithFlag {
            content: part,
            end_stream: EndStream::Yes,
        })))
    }

    pub fn once_bytes<B>(bytes: B) -> DataOrHeadersWithFlagStream
    where
        B: Into<Bytes>,
    {
        DataOrHeadersWithFlagStream::once(DataOrHeaders::Data(bytes.into()))
    }

    // getters

    /// Create a stream without "last" flag
    pub fn drop_last_flag(self) -> TryStreamBox<DataOrHeaders> {
        Box::pin(self.map_ok(|DataOrHeadersWithFlag { content, .. }| content))
    }

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> TryStreamBox<Bytes> {
        Box::pin(
            self.try_filter_map(|DataOrHeadersWithFlag { content, .. }| {
                future::ok(match content {
                    DataOrHeaders::Data(data) => Some(data),
                    _ => None,
                })
            }),
        )
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(AssertUnwindSafe(self.0).catch_unwind().then(|r| {
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

impl Stream for DataOrHeadersWithFlagStream {
    type Item = crate::Result<DataOrHeadersWithFlag>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}
