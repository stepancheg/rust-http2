use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use futures::future;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::task::Context;
use futures::TryFutureExt;
use futures::TryStreamExt;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use crate::message::SimpleHttpMessage;
use crate::solicit::header::Headers;
use crate::solicit_async::*;
use crate::stream_after_headers::HttpStreamAfterHeaders;

/// Convenient wrapper around async HTTP response future/stream
pub struct ClientResponseFuture(pub HttpFutureSend<(Headers, HttpStreamAfterHeaders)>);

impl ClientResponseFuture {
    // constructors

    pub fn new<F>(future: F) -> ClientResponseFuture
    where
        F: Future<Output = crate::Result<(Headers, HttpStreamAfterHeaders)>> + Send + 'static,
    {
        ClientResponseFuture(Box::pin(future))
    }

    pub fn from_stream<S>(mut stream: S) -> ClientResponseFuture
    where
        S: Stream<Item = crate::Result<DataOrHeadersWithFlag>> + Unpin + Send + 'static,
    {
        ClientResponseFuture::new(async move {
            // Check that first frame is HEADERS
            let (first, rem) = match stream.try_next().await? {
                Some(part) => match part.content {
                    DataOrHeaders::Headers(headers) => {
                        (headers, HttpStreamAfterHeaders::from_parts(stream))
                    }
                    DataOrHeaders::Data(..) => {
                        return Err(crate::Error::InvalidFrame("data before headers".to_owned()))
                    }
                },
                None => {
                    return Err(crate::Error::InvalidFrame(
                        "empty response, expecting headers".to_owned(),
                    ))
                }
            };
            Ok((first, rem))
        })
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
                    future::ok::<_, crate::Error>(c)
                }),
        )
    }
}

impl Future for ClientResponseFuture {
    type Output = crate::Result<(Headers, HttpStreamAfterHeaders)>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<crate::Result<(Headers, HttpStreamAfterHeaders)>> {
        Pin::new(&mut self.0).poll(cx)
    }
}
