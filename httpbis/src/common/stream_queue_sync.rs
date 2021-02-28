use std::marker;
use std::pin::Pin;
use std::task::Poll;

use futures::channel::mpsc::unbounded;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;

use crate::common::types::Types;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::server::stream_handler::ServerRequestStreamHandler;
use crate::server::types::ServerTypes;
use crate::solicit::end_stream::EndStream;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;
use futures::task::Context;

pub(crate) struct StreamQueueSyncSender<T: Types> {
    sender: UnboundedSender<Result<DataOrTrailers, crate::Error>>,
    _marker: marker::PhantomData<T>,
}

pub(crate) struct StreamQueueSyncReceiver<T: Types> {
    receiver: UnboundedReceiver<Result<DataOrTrailers, crate::Error>>,
    eof_received: bool,
    conn_died: SomethingDiedErrorHolder<ConnDiedType>,
    _marker: marker::PhantomData<T>,
}

impl<T: Types> StreamQueueSyncSender<T> {
    pub fn send(&self, item: Result<DataOrTrailers, crate::Error>) -> crate::Result<()> {
        if let Err(_send_error) = self.sender.unbounded_send(item) {
            Err(crate::Error::PullStreamDied)
        } else {
            Ok(())
        }
    }
}

impl ServerRequestStreamHandler for StreamQueueSyncSender<ServerTypes> {
    fn data_frame(&mut self, data: Bytes, end_stream: EndStream) -> crate::Result<()> {
        self.send(Ok(DataOrTrailers::Data(data, end_stream)))
    }

    fn trailers(self: Box<Self>, trailers: Headers) -> crate::Result<()> {
        self.send(Ok(DataOrTrailers::Trailers(trailers)))
    }

    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()> {
        self.send(Err(crate::Error::RstStreamReceived(error_code)))
    }

    fn error(self: Box<Self>, error: crate::Error) -> crate::Result<()> {
        self.send(Err(error))
    }
}

impl<T: Types> Stream for StreamQueueSyncReceiver<T> {
    type Item = crate::Result<DataOrTrailers>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrTrailers>>> {
        if self.eof_received {
            return Poll::Ready(None);
        }

        let part = match Pin::new(&mut self.receiver).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => {
                // should be impossible, because
                // callbacks are notified of client death in
                // `HttpStreamCommon::conn_died`
                return Poll::Ready(Some(Err(self.conn_died.error())));
            }
            Poll::Ready(Some(Err(e))) => {
                self.eof_received = true;
                return Poll::Ready(Some(Err(e)));
            }
            Poll::Ready(Some(Ok(part))) => {
                if part.end_stream() == EndStream::Yes {
                    self.eof_received = true;
                }
                part
            }
        };

        Poll::Ready(Some(Ok(part)))
    }
}

pub(crate) fn stream_queue_sync<T: Types>(
    conn_died: SomethingDiedErrorHolder<ConnDiedType>,
) -> (StreamQueueSyncSender<T>, StreamQueueSyncReceiver<T>) {
    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender {
        sender: utx,
        _marker: marker::PhantomData,
    };
    let rx = StreamQueueSyncReceiver {
        receiver: urx,
        eof_received: false,
        conn_died,
        _marker: marker::PhantomData,
    };

    (tx, rx)
}
