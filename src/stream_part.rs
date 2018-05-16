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
pub enum HttpStreamPartAfterHeaders {
    /// DATA frame
    Data(Bytes, EndStream),
    /// HEADERS frame with END_STREAM flag
    Trailers(Headers),
}

impl HttpStreamPartAfterHeaders {
    pub fn intermediate_data(data: Bytes) -> Self {
        HttpStreamPartAfterHeaders::Data(data, EndStream::No)
    }

    pub fn into_part(self) -> DataOrHeadersWithFlag {
        match self {
            HttpStreamPartAfterHeaders::Data(data, end_stream) => {
                DataOrHeadersWithFlag {
                    content: DataOrHeaders::Data(data),
                    last: end_stream == EndStream::Yes,
                }
            }
            HttpStreamPartAfterHeaders::Trailers(headers) => {
                DataOrHeadersWithFlag {
                    content: DataOrHeaders::Headers(headers),
                    last: true,
                }
            }
        }
    }
}


/// Stream of DATA of HEADER frames after initial HEADER
pub struct HttpPartStreamAfterHeaders(
    pub HttpFutureStreamSend<HttpStreamPartAfterHeaders>
);

impl HttpPartStreamAfterHeaders {
    // constructors

    pub fn new<S>(s: S) -> HttpPartStreamAfterHeaders
        where S : Stream<Item=HttpStreamPartAfterHeaders, Error=error::Error> + Send + 'static
    {
        HttpPartStreamAfterHeaders(Box::new(s))
    }

    pub fn from_parts<S>(s: S) -> HttpPartStreamAfterHeaders
        where S : Stream<Item=DataOrHeadersWithFlag, Error=error::Error> + Send + 'static
    {
        HttpPartStreamAfterHeaders::new(s.map(DataOrHeadersWithFlag::into_after_headers))
    }

    pub fn empty() -> HttpPartStreamAfterHeaders {
        HttpPartStreamAfterHeaders::new(stream::empty())
    }

    pub fn bytes<S>(bytes: S) -> HttpPartStreamAfterHeaders
        where S : Stream<Item=Bytes, Error=error::Error> + Send + 'static
    {
        HttpPartStreamAfterHeaders::new(bytes.map(HttpStreamPartAfterHeaders::intermediate_data))
    }

    pub fn once(part: DataOrHeaders) -> HttpPartStreamAfterHeaders {
        let part = match part {
            DataOrHeaders::Data(data) => {
                HttpStreamPartAfterHeaders::Data(data, EndStream::Yes)
            },
            DataOrHeaders::Headers(header) => {
                HttpStreamPartAfterHeaders::Trailers(header)
            },
        };
        HttpPartStreamAfterHeaders::new(stream::once(Ok(part)))
    }

    pub fn once_bytes<B>(bytes: B) -> HttpPartStreamAfterHeaders
        where B : Into<Bytes>
    {
        HttpPartStreamAfterHeaders::once(DataOrHeaders::Data(bytes.into()))
    }

    // getters

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> impl Stream<Item=Bytes, Error=error::Error> + Send {
        self.filter_map(|p| {
            match p {
                HttpStreamPartAfterHeaders::Data(data, ..) => Some(data),
                HttpStreamPartAfterHeaders::Trailers(..) => None,
            }
        })
    }

    pub fn into_flag_stream(self) -> impl Stream<Item=DataOrHeadersWithFlag, Error=error::Error> + Send {
        self.0.map(HttpStreamPartAfterHeaders::into_part)
    }

    pub fn into_part_stream(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(self.into_flag_stream())
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> HttpPartStreamAfterHeaders {
        HttpPartStreamAfterHeaders::new(panic::AssertUnwindSafe(self.0).catch_unwind().then(|r| {
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

impl Stream for HttpPartStreamAfterHeaders {
    type Item = HttpStreamPartAfterHeaders;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll()
    }
}
