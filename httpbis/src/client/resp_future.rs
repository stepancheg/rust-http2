use std::mem;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures::future;
use futures::stream;
use futures::Future;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use tokio::sync::oneshot;

use crate::client::handler::ClientResponseStreamHandler;
use crate::client::increase_in_window::ClientIncreaseInWindow;
use crate::client::types::ClientTypes;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::common::stream_queue_sync::StreamQueueSyncSender;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlagStream;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::solicit::end_stream::EndStream;
use crate::solicit_async::HttpFutureStreamSend;
use crate::stream_after_headers::HttpStreamAfterHeaders;
use crate::stream_after_headers::HttpStreamAfterHeadersBox;
use crate::stream_after_headers::HttpStreamAfterHeadersEmpty;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use crate::SimpleHttpMessage;

pub struct ClientResponseFutureImpl {
    rx: oneshot::Receiver<crate::Result<HeadersReceivedMessage>>,
    conn_died: SomethingDiedErrorHolder<ConnDiedType>,
}

impl ClientResponseFutureImpl {
    pub(crate) fn new(
        increase_in_window: ClientIncreaseInWindow,
        conn_died: SomethingDiedErrorHolder<ConnDiedType>,
    ) -> (impl ClientResponseStreamHandler, ClientResponseFutureImpl) {
        let (tx, rx) = oneshot::channel();
        (
            ClientResponseStreamHandlerImpl::WaitingForHeaders(
                tx,
                conn_died.clone(),
                increase_in_window,
            ),
            ClientResponseFutureImpl { rx, conn_died },
        )
    }
}

impl Future for ClientResponseFutureImpl {
    type Output = crate::Result<(Headers, HttpStreamAfterHeadersBox)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        match Pin::new(&mut me.rx).poll(cx) {
            Poll::Ready(Ok(Ok(HeadersReceivedMessage { headers, body }))) => {
                Poll::Ready(Ok((headers, body)))
            }
            Poll::Ready(Ok(Err(e))) => Poll::Ready(Err(e)),
            Poll::Ready(Err(_)) => Poll::Ready(Err(me.conn_died.error())),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct HeadersReceivedMessage {
    headers: Headers,
    body: HttpStreamAfterHeadersBox,
}

enum ClientResponseStreamHandlerImpl {
    WaitingForHeaders(
        oneshot::Sender<crate::Result<HeadersReceivedMessage>>,
        SomethingDiedErrorHolder<ConnDiedType>,
        ClientIncreaseInWindow,
    ),
    Body(StreamQueueSyncSender<ClientTypes>),
    Stone,
}

impl ClientResponseStreamHandler for ClientResponseStreamHandlerImpl {
    fn headers(&mut self, headers: Headers, end_stream: EndStream) -> crate::Result<()> {
        match mem::replace(self, ClientResponseStreamHandlerImpl::Stone) {
            ClientResponseStreamHandlerImpl::WaitingForHeaders(
                tx,
                conn_died,
                increase_in_window,
            ) => {
                let body: HttpStreamAfterHeadersBox = match end_stream {
                    EndStream::Yes => Box::pin(HttpStreamAfterHeadersEmpty),
                    EndStream::No => {
                        let (tx, rx) = stream_queue_sync(conn_died);
                        *self = ClientResponseStreamHandlerImpl::Body(tx);
                        Box::pin(StreamFromNetwork::new(rx, increase_in_window.0))
                    }
                };
                match tx.send(Ok(HeadersReceivedMessage { headers, body })) {
                    Ok(()) => Ok(()),
                    Err(_) => Err(crate::Error::CallerDied),
                }
            }
            _ => panic!("wrong state"),
        }
    }

    fn data_frame(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()> {
        match self {
            ClientResponseStreamHandlerImpl::WaitingForHeaders(..) => panic!("incorrect state"),
            ClientResponseStreamHandlerImpl::Body(tx) => {
                tx.send(Ok(DataOrTrailers::Data(data, end_stream)))
            }
            ClientResponseStreamHandlerImpl::Stone => panic!("incorrect state"),
        }
    }

    fn trailers(self: Box<Self>, trailers: Headers) -> crate::Result<()> {
        match *self {
            ClientResponseStreamHandlerImpl::WaitingForHeaders(..) => panic!("incorrect state"),
            ClientResponseStreamHandlerImpl::Body(tx) => {
                tx.send(Ok(DataOrTrailers::Trailers(trailers)))
            }
            ClientResponseStreamHandlerImpl::Stone => panic!("incorrect state"),
        }
    }

    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()> {
        self.error(crate::Error::RstStreamReceived(error_code))
    }

    fn error(self: Box<Self>, error: crate::Error) -> crate::Result<()> {
        match *self {
            ClientResponseStreamHandlerImpl::WaitingForHeaders(tx, ..) => tx
                .send(Err(error))
                .map_err(|_| crate::Error::PullStreamDied),
            ClientResponseStreamHandlerImpl::Body(tx) => tx
                .send(Err(error))
                .map_err(|_| crate::Error::PullStreamDied),
            ClientResponseStreamHandlerImpl::Stone => Ok(()),
        }
    }
}

pub struct ClientResponseFuture3(
    Pin<
        Box<
            dyn Future<Output = crate::Result<(Headers, HttpStreamAfterHeadersBox)>>
                + Send
                + 'static,
        >,
    >,
);

impl ClientResponseFuture3 {
    pub fn new(
        future: impl Future<Output = crate::Result<(Headers, HttpStreamAfterHeadersBox)>>
            + Send
            + 'static,
    ) -> ClientResponseFuture3 {
        ClientResponseFuture3(Box::pin(future))
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
                    let rem = rem.into_stream_with_flag();
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

    pub fn collect(self) -> impl Future<Output = crate::Result<SimpleHttpMessage>> + 'static {
        Box::pin(
            self.into_stream()
                .try_fold(SimpleHttpMessage::new(), |mut c, p| {
                    c.add(p);
                    future::ok::<_, crate::Error>(c)
                }),
        )
    }
}

impl Future for ClientResponseFuture3 {
    type Output = crate::Result<(Headers, HttpStreamAfterHeadersBox)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(cx)
    }
}
