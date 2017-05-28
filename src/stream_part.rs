use std::io;
use std::panic;

use futures::Poll;
use futures::stream;
use futures::stream::Stream;

use bytes::Bytes;

use error;

use solicit::header::Headers;

use solicit_async::*;

use misc::any_to_string;


/// Stream frame content
#[derive(Debug)]
pub enum HttpStreamPartContent {
    /// HEADERS frame
    Headers(Headers),
    /// DATA frame
    Data(Bytes),
}

/// Stream frame content with END_STREAM flag
pub struct HttpStreamPart {
    pub content: HttpStreamPartContent,
    /// END_STREAM
    pub last: bool,
}

impl HttpStreamPart {
    pub fn last_headers(headers: Headers) -> Self {
        HttpStreamPart {
            content: HttpStreamPartContent::Headers(headers),
            last: true,
        }
    }

    pub fn intermediate_headers(headers: Headers) -> Self {
        HttpStreamPart {
            content: HttpStreamPartContent::Headers(headers),
            last: false,
        }
    }

    pub fn intermediate_data(data: Bytes) -> Self {
        HttpStreamPart {
            content: HttpStreamPartContent::Data(data),
            last: false,
        }
    }

    pub fn last_data(data: Bytes) -> Self {
        HttpStreamPart {
            content: HttpStreamPartContent::Data(data),
            last: true,
        }
    }

}


/// Stream of DATA of HEADER frames
pub struct HttpPartStream(
    pub HttpFutureStreamSend<HttpStreamPart>
);

impl HttpPartStream {
    // constructors

    pub fn new<S>(s: S) -> HttpPartStream
        where S : Stream<Item=HttpStreamPart, Error=error::Error> + Send + 'static
    {
        HttpPartStream(Box::new(s))
    }

    pub fn empty() -> HttpPartStream {
        HttpPartStream::new(stream::empty())
    }

    pub fn bytes<S>(bytes: S) -> HttpPartStream
        where S : Stream<Item=Bytes, Error=error::Error> + Send + 'static
    {
        HttpPartStream::new(bytes.map(HttpStreamPart::intermediate_data))
    }

    pub fn once(part: HttpStreamPartContent) -> HttpPartStream {
        HttpPartStream::new(stream::once(Ok(HttpStreamPart { content: part, last: true })))
    }

    pub fn once_bytes<B>(bytes: B) -> HttpPartStream
        where B : Into<Bytes>
    {
        HttpPartStream::once(HttpStreamPartContent::Data(bytes.into()))
    }

    // getters

    /// Create a stream without "last" flag
    pub fn drop_last_flag(self) -> HttpFutureStreamSend<HttpStreamPartContent> {
        Box::new(self.map(|HttpStreamPart { content, .. }| content))
    }

    /// Take only `DATA` frames from the stream
    pub fn filter_data(self) -> HttpFutureStreamSend<Bytes> {
        Box::new(self.filter_map(|HttpStreamPart { content, .. }| {
            match content {
                HttpStreamPartContent::Data(data) => Some(data),
                _ => None,
            }
        }))
    }

    /// Take only `DATA` frames, return an error on header frames
    pub fn check_only_data(self) -> HttpFutureStreamSend<Bytes> {
        Box::new(self.and_then(|HttpStreamPart { content, .. }| {
            match content {
                HttpStreamPartContent::Data(data) => {
                    Ok(data)
                },
                HttpStreamPartContent::Headers(..) => {
                    Err(error::Error::from(io::Error::new(io::ErrorKind::Other, "expecting only DATA frames")))
                },
            }
        }))
    }

    /// Wrap a stream with `catch_unwind` combinator.
    /// Transform panic into `error::Error`
    pub fn catch_unwind(self) -> HttpPartStream {
        HttpPartStream::new(panic::AssertUnwindSafe(self.0).catch_unwind().then(|r| {
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

impl Stream for HttpPartStream {
    type Item = HttpStreamPart;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll()
    }
}
