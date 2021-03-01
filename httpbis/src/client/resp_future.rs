use std::mem;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use futures::Future;
use tokio::sync::oneshot;

use crate::client::handler::ClientResponseStreamHandler;
use crate::client::types::ClientTypes;
use crate::common::increase_in_window_common::IncreaseInWindowCommon;
use crate::common::stream_after_headers::HttpStreamAfterHeadersEmpty;
use crate::common::stream_after_headers::StreamAfterHeadersBox;
use crate::common::stream_from_network::StreamFromNetwork;
use crate::common::stream_queue_sync::stream_queue_sync;
use crate::common::stream_queue_sync::StreamQueueSyncSender;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::solicit::end_stream::EndStream;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;

pub struct ClientResponseFutureImpl {
    rx: oneshot::Receiver<crate::Result<HeadersReceivedMessage>>,
    conn_died: SomethingDiedErrorHolder<ConnDiedType>,
}

impl ClientResponseFutureImpl {
    pub(crate) fn new(
        increase_in_window: IncreaseInWindowCommon<ClientTypes>,
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
    type Output = crate::Result<(Headers, StreamAfterHeadersBox)>;

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
    body: StreamAfterHeadersBox,
}

enum ClientResponseStreamHandlerImpl {
    WaitingForHeaders(
        oneshot::Sender<crate::Result<HeadersReceivedMessage>>,
        SomethingDiedErrorHolder<ConnDiedType>,
        IncreaseInWindowCommon<ClientTypes>,
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
                let body: StreamAfterHeadersBox = match end_stream {
                    EndStream::Yes => Box::pin(HttpStreamAfterHeadersEmpty),
                    EndStream::No => {
                        let (tx, rx) = stream_queue_sync(conn_died);
                        *self = ClientResponseStreamHandlerImpl::Body(tx);
                        Box::pin(StreamFromNetwork::new(rx, increase_in_window))
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
