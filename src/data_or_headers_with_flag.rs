use futures::stream::Stream;
use futures::stream;

use bytes::Bytes;

use data_or_headers::DataOrHeaders;
use solicit::header::Headers;
use solicit::connection::EndStream;
use solicit_async::HttpFutureStreamSend;
use data_or_trailers::DataOrTrailers;
use error;
use std::panic::AssertUnwindSafe;
use misc::any_to_string;
use futures::Poll;


/// Stream frame content with END_STREAM flag
pub struct DataOrHeadersWithFlag {
    pub content: DataOrHeaders,
    /// END_STREAM
    pub last: bool,
}

impl DataOrHeadersWithFlag {
    pub fn last_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            last: true,
        }
    }

    pub fn intermediate_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            last: false,
        }
    }

    pub fn intermediate_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: false,
        }
    }

    pub fn last_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: true,
        }
    }

    pub fn into_after_headers(self) -> DataOrTrailers {
        let DataOrHeadersWithFlag { content, last } = self;
        match (content, last) {
            (DataOrHeaders::Data(data), last) => {
                let end_stream = if last { EndStream::Yes } else { EndStream::No };
                DataOrTrailers::Data(data, end_stream)
            }
            (DataOrHeaders::Headers(headers), _) => {
                DataOrTrailers::Trailers(headers)
            }
        }
    }
}


/// Stream of DATA of HEADER frames
pub struct DataOrHeadersWithFlagStream(
    pub HttpFutureStreamSend<DataOrHeadersWithFlag>
);

impl DataOrHeadersWithFlagStream {
    // constructors

    pub fn new<S>(s: S) -> DataOrHeadersWithFlagStream
        where S : Stream<Item=DataOrHeadersWithFlag, Error=error::Error> + Send + 'static
    {
        DataOrHeadersWithFlagStream(Box::new(s))
    }

    pub fn empty() -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(stream::empty())
    }

    pub fn bytes<S>(bytes: S) -> DataOrHeadersWithFlagStream
        where S : Stream<Item=Bytes, Error=error::Error> + Send + 'static
    {
        DataOrHeadersWithFlagStream::new(bytes.map(DataOrHeadersWithFlag::intermediate_data))
    }

    pub fn once(part: DataOrHeaders) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(stream::once(Ok(DataOrHeadersWithFlag { content: part, last: true })))
    }

    pub fn once_bytes<B>(bytes: B) -> DataOrHeadersWithFlagStream
        where B : Into<Bytes>
    {
        DataOrHeadersWithFlagStream::once(DataOrHeaders::Data(bytes.into()))
    }

    // getters

    /// Create a stream without "last" flag
    pub fn drop_last_flag(self) -> HttpFutureStreamSend<DataOrHeaders> {
        Box::new(self.map(|DataOrHeadersWithFlag { content, .. }| content))
    }

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> HttpFutureStreamSend<Bytes> {
        Box::new(self.filter_map(|DataOrHeadersWithFlag { content, .. }| {
            match content {
                DataOrHeaders::Data(data) => Some(data),
                _ => None,
            }
        }))
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(AssertUnwindSafe(self.0).catch_unwind().then(|r| {
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

impl Stream for DataOrHeadersWithFlagStream {
    type Item = DataOrHeadersWithFlag;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll()
    }
}
