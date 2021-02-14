use futures::future;
use futures::TryFutureExt;
use futures::TryStreamExt;

use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use std::future::Future;

use bytes::Bytes;

use crate::message::SimpleHttpMessage;
use crate::solicit::header::Headers;
use crate::solicit_async::*;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use crate::data_or_trailers::*;
use crate::error;
use crate::result;
use futures::task::Context;
use std::pin::Pin;
use std::task::Poll;

/// Convenient wrapper around async HTTP response future/stream
pub struct Response(pub HttpFutureSend<(Headers, HttpStreamAfterHeaders)>);

impl Response {
    // constructors

    pub fn new<F>(future: F) -> Response
    where
        F: Future<Output = result::Result<(Headers, HttpStreamAfterHeaders)>> + Send + 'static,
    {
        Response(Box::pin(future))
    }

    pub fn headers_and_stream(headers: Headers, stream: HttpStreamAfterHeaders) -> Response {
        Response::new(future::ok((headers, stream)))
    }

    pub fn headers_and_bytes_stream<S>(headers: Headers, content: S) -> Response
    where
        S: Stream<Item = result::Result<Bytes>> + Send + 'static,
    {
        Response::headers_and_stream(headers, HttpStreamAfterHeaders::bytes(content))
    }

    /// Create a response with only headers
    pub fn headers(headers: Headers) -> Response {
        Response::headers_and_bytes_stream(headers, stream::empty())
    }

    /// Create a response with headers and response body
    pub fn headers_and_bytes<B: Into<Bytes>>(header: Headers, content: B) -> Response {
        Response::headers_and_bytes_stream(header, stream::once(future::ok(content.into())))
    }

    pub fn message(message: SimpleHttpMessage) -> Response {
        Response::headers_and_bytes(message.headers, message.body)
    }

    pub fn found_200_plain_text(body: &str) -> Response {
        Response::message(SimpleHttpMessage::found_200_plain_text(body))
    }

    pub fn not_found_404() -> Response {
        Response::headers(Headers::not_found_404())
    }

    pub fn redirect_302(location: &str) -> Response {
        let mut headers = Headers::new_status(302);
        headers.add("location", location);
        Response::headers(headers)
    }

    pub fn from_stream<S>(mut stream: S) -> Response
    where
        S: Stream<Item = result::Result<DataOrHeadersWithFlag>> + Unpin + Send + 'static,
    {
        Response::new(async move {
            // Check that first frame is HEADERS
            let (first, rem) = match stream.try_next().await? {
                Some(part) => match part.content {
                    DataOrHeaders::Headers(headers) => {
                        (headers, HttpStreamAfterHeaders::from_parts(stream))
                    }
                    DataOrHeaders::Data(..) => {
                        return Err(error::Error::InvalidFrame("data before headers".to_owned()))
                    }
                },
                None => {
                    return Err(error::Error::InvalidFrame(
                        "empty response, expecting headers".to_owned(),
                    ))
                }
            };
            Ok((first, rem))
        })
    }

    pub fn err(err: error::Error) -> Response {
        Response::new(future::err(err))
    }

    // getters

    pub fn into_stream_flag(self) -> HttpFutureStreamSend<DataOrHeadersWithFlag> {
        Box::pin(
            self.0
                .map_ok(|(headers, rem)| {
                    // NOTE: flag may be wrong for first item
                    let header = stream::once(future::ok(
                        DataOrHeadersWithFlag::intermediate_headers(headers),
                    ));
                    let rem = rem.into_flag_stream();
                    header.chain(rem)
                })
                .try_flatten_stream(),
        )
    }

    pub fn into_stream(self) -> HttpFutureStreamSend<DataOrHeaders> {
        Box::pin(TryStreamExt::map_ok(self.into_stream_flag(), |c| c.content))
    }

    pub fn into_part_stream(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(self.into_stream_flag())
    }

    pub fn collect(self) -> HttpFutureSend<SimpleHttpMessage> {
        Box::pin(
            self.into_stream()
                .try_fold(SimpleHttpMessage::new(), |mut c, p| {
                    c.add(p);
                    future::ok::<_, error::Error>(c)
                }),
        )
    }
}

impl Future for Response {
    type Output = result::Result<(Headers, HttpStreamAfterHeaders)>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<result::Result<(Headers, HttpStreamAfterHeaders)>> {
        Pin::new(&mut self.0).poll(cx)
    }
}
