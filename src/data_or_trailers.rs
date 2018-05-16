use std::panic;

use futures::Poll;
use futures::stream;
use futures::stream::Stream;

use bytes::Bytes;

use error;

use solicit::header::Headers;

use solicit_async::*;

use misc::any_to_string;
use solicit::connection::EndStream;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use data_or_headers_with_flag::DataOrHeadersWithFlagStream;


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
            DataOrTrailers::Data(data, end_stream) => {
                DataOrHeadersWithFlag {
                    content: DataOrHeaders::Data(data),
                    last: end_stream == EndStream::Yes,
                }
            }
            DataOrTrailers::Trailers(headers) => {
                DataOrHeadersWithFlag {
                    content: DataOrHeaders::Headers(headers),
                    last: true,
                }
            }
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
    pub HttpFutureStreamSend<DataOrTrailers>
);

impl HttpStreamAfterHeaders {
    // constructors

    /// Box and wraper futures stream of `DataOrTrailers`.
    pub fn new<S>(s: S) -> HttpStreamAfterHeaders
        where S : Stream<Item=DataOrTrailers, Error=error::Error> + Send + 'static
    {
        HttpStreamAfterHeaders(Box::new(s))
    }

    pub(crate) fn from_parts<S>(s: S) -> HttpStreamAfterHeaders
        where S : Stream<Item=DataOrHeadersWithFlag, Error=error::Error> + Send + 'static
    {
        HttpStreamAfterHeaders::new(s.map(DataOrHeadersWithFlag::into_after_headers))
    }

    /// Create an empty response stream (no body, no trailers).
    pub fn empty() -> HttpStreamAfterHeaders {
        HttpStreamAfterHeaders::new(stream::empty())
    }

    /// Create a response from a stream of bytes.
    pub fn bytes<S>(bytes: S) -> HttpStreamAfterHeaders
        where S : Stream<Item=Bytes, Error=error::Error> + Send + 'static
    {
        HttpStreamAfterHeaders::new(bytes.map(DataOrTrailers::intermediate_data))
    }

    pub fn once(part: DataOrHeaders) -> HttpStreamAfterHeaders {
        let part = match part {
            DataOrHeaders::Data(data) => {
                DataOrTrailers::Data(data, EndStream::Yes)
            },
            DataOrHeaders::Headers(header) => {
                DataOrTrailers::Trailers(header)
            },
        };
        HttpStreamAfterHeaders::new(stream::once(Ok(part)))
    }

    /// Create simple response stream from bytes.
    ///
    /// Useful when bytes are available and length is small.
    ///
    /// If bytes object is large, it will be split into multiple frames.
    pub fn once_bytes<B>(bytes: B) -> HttpStreamAfterHeaders
        where B : Into<Bytes>
    {
        HttpStreamAfterHeaders::once(DataOrHeaders::Data(bytes.into()))
    }

    // getters

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> impl Stream<Item=Bytes, Error=error::Error> + Send {
        self.filter_map(|p| {
            match p {
                DataOrTrailers::Data(data, ..) => Some(data),
                DataOrTrailers::Trailers(..) => None,
            }
        })
    }

    pub(crate) fn into_flag_stream(self) -> impl Stream<Item=DataOrHeadersWithFlag, Error=error::Error> + Send {
        self.0.map(DataOrTrailers::into_part)
    }

    pub(crate) fn into_part_stream(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(self.into_flag_stream())
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> HttpStreamAfterHeaders {
        HttpStreamAfterHeaders::new(panic::AssertUnwindSafe(self.0).catch_unwind().then(|r| {
            match r {
                Ok(r) => r,
                Err(e) => {
                    let e = any_to_string(e);
                    // TODO: send plain text error if headers weren't sent yet
                    warn!("handler panicked: {}", e);
                    Err(error::Error::HandlerPanicked(e))
                },
            }
        }))
    }
}

impl Stream for HttpStreamAfterHeaders {
    type Item = DataOrTrailers;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll()
    }
}
