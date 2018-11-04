#![allow(dead_code)]

use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
use futures::Async;
use futures::Poll;

use error;

use bytes::Bytes;
use client_died_error_holder::*;
use common::stream_handler::StreamHandler;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use result;
use ErrorCode;
use Headers;

pub struct StreamQueueSyncSender {
    sender: UnboundedSender<Result<DataOrHeadersWithFlag, error::Error>>,
}

pub struct StreamQueueSyncReceiver {
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
    receiver: UnboundedReceiver<Result<DataOrHeadersWithFlag, error::Error>>,
    eof_received: bool,
}

impl StreamQueueSyncSender {
    fn send(&self, item: Result<DataOrHeadersWithFlag, error::Error>) -> result::Result<()> {
        if let Err(_send_error) = self.sender.unbounded_send(item) {
            // TODO: better error
            Err(error::Error::Other("pull stream died"))
        } else {
            Ok(())
        }
    }
}

impl StreamHandler for StreamQueueSyncSender {
    fn headers(&mut self, headers: Headers, end_stream: bool) -> result::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            last: end_stream,
        }))
    }

    fn data_frame(&mut self, data: Bytes, end_stream: bool) -> result::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: end_stream,
        }))
    }

    fn trailers(&mut self, trailers: Headers) -> result::Result<()> {
        self.send(Ok(DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(trailers),
            last: true,
        }))
    }

    fn rst(&mut self, error_code: ErrorCode) -> result::Result<()> {
        self.send(Err(error::Error::RstStreamReceived(error_code)))
    }

    fn error(&mut self, error: error::Error) -> result::Result<()> {
        self.send(Err(error))
    }
}

impl Stream for StreamQueueSyncReceiver {
    type Item = DataOrHeadersWithFlag;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<DataOrHeadersWithFlag>, error::Error> {
        if self.eof_received {
            return Ok(Async::Ready(None));
        }

        let part = match self.receiver.poll() {
            Err(()) => unreachable!(),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(None)) => return Err(self.conn_died_error_holder.error()),
            Ok(Async::Ready(Some(Err(e)))) => {
                self.eof_received = true;
                return Err(e);
            }
            Ok(Async::Ready(Some(Ok(part)))) => {
                if part.last {
                    self.eof_received = true;
                }
                part
            }
        };

        Ok(Async::Ready(Some(part)))
    }
}

pub(crate) fn stream_queue_sync(
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
) -> (StreamQueueSyncSender, StreamQueueSyncReceiver) {
    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender { sender: utx };
    let rx = StreamQueueSyncReceiver {
        conn_died_error_holder,
        receiver: urx,
        eof_received: false,
    };

    (tx, rx)
}
