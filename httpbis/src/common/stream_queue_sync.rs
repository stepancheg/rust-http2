#![allow(dead_code)]

use futures::channel::mpsc::unbounded;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;
use std::task::Poll;

use crate::error;

use crate::client::stream_handler::ClientResponseStreamHandler;
use crate::client::types::ClientTypes;
use crate::common::types::Types;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::server::stream_handler::ServerRequestStreamHandler;
use crate::server::types::ServerTypes;
use crate::ErrorCode;
use crate::Headers;
use bytes::Bytes;
use futures::task::Context;
use std::marker;
use std::pin::Pin;

pub(crate) struct StreamQueueSyncSender<T: Types> {
    sender: UnboundedSender<Result<DataOrHeadersWithFlag, error::Error>>,
    _marker: marker::PhantomData<T>,
}

pub(crate) struct StreamQueueSyncReceiver<T: Types> {
    receiver: UnboundedReceiver<Result<DataOrHeadersWithFlag, error::Error>>,
    eof_received: bool,
    _marker: marker::PhantomData<T>,
}

impl<T: Types> StreamQueueSyncSender<T> {
    fn send(&self, item: Result<DataOrHeadersWithFlag, error::Error>) -> crate::Result<()> {
        if let Err(_send_error) = self.sender.unbounded_send(item) {
            // TODO: better error
            Err(error::Error::PullStreamDied)
        } else {
            Ok(())
        }
    }
}

impl ServerRequestStreamHandler for StreamQueueSyncSender<ServerTypes> {
    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: end_stream,
        }))
    }

    fn trailers(&mut self, trailers: Headers) -> crate::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(trailers),
            last: true,
        }))
    }

    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()> {
        self.send(Err(error::Error::RstStreamReceived(error_code)))
    }

    fn error(self: Box<Self>, error: error::Error) -> crate::Result<()> {
        self.send(Err(error))
    }
}

impl ClientResponseStreamHandler for StreamQueueSyncSender<ClientTypes> {
    fn headers(&mut self, headers: Headers, end_stream: bool) -> crate::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            last: end_stream,
        }))
    }

    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> crate::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: end_stream,
        }))
    }

    fn trailers(&mut self, trailers: Headers) -> crate::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(trailers),
            last: true,
        }))
    }

    fn rst(self: Box<Self>, error_code: ErrorCode) -> crate::Result<()> {
        self.send(Err(error::Error::RstStreamReceived(error_code)))
    }

    fn error(self: Box<Self>, error: error::Error) -> crate::Result<()> {
        self.send(Err(error))
    }
}

impl<T: Types> Stream for StreamQueueSyncReceiver<T> {
    type Item = crate::Result<DataOrHeadersWithFlag>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<crate::Result<DataOrHeadersWithFlag>>> {
        if self.eof_received {
            return Poll::Ready(None);
        }

        let part = match Pin::new(&mut self.receiver).poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => {
                // should be impossible, because
                // callbacks are notified of client death in
                // `HttpStreamCommon::conn_died`
                return Poll::Ready(Some(Err(error::Error::InternalError(
                    "internal error: unexpected EOF".to_owned(),
                ))));
            }
            Poll::Ready(Some(Err(e))) => {
                self.eof_received = true;
                return Poll::Ready(Some(Err(e)));
            }
            Poll::Ready(Some(Ok(part))) => {
                if part.last {
                    self.eof_received = true;
                }
                part
            }
        };

        Poll::Ready(Some(Ok(part)))
    }
}

pub(crate) fn stream_queue_sync<T: Types>() -> (StreamQueueSyncSender<T>, StreamQueueSyncReceiver<T>)
{
    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender {
        sender: utx,
        _marker: marker::PhantomData,
    };
    let rx = StreamQueueSyncReceiver {
        receiver: urx,
        eof_received: false,
        _marker: marker::PhantomData,
    };

    (tx, rx)
}
