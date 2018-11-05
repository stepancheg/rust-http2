#![allow(dead_code)]

use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
use futures::Async;
use futures::Poll;

use error;

use bytes::Bytes;
use client::stream_handler::ClientStreamHandler;
use client::types::ClientTypes;
use client_died_error_holder::*;
use common::types::Types;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use result;
use server::stream_handler::ServerStreamHandler;
use server::types::ServerTypes;
use std::marker;
use ClientRequest;
use ErrorCode;
use Headers;
use Response;

pub(crate) struct StreamQueueSyncSender<T: Types> {
    sender: UnboundedSender<Result<DataOrHeadersWithFlag, error::Error>>,
    _marker: marker::PhantomData<T>,
}

pub(crate) struct StreamQueueSyncReceiver<T: Types> {
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
    receiver: UnboundedReceiver<Result<DataOrHeadersWithFlag, error::Error>>,
    eof_received: bool,
    _marker: marker::PhantomData<T>,
}

impl<T: Types> StreamQueueSyncSender<T> {
    fn send(&self, item: Result<DataOrHeadersWithFlag, error::Error>) -> result::Result<()> {
        if let Err(_send_error) = self.sender.unbounded_send(item) {
            // TODO: better error
            Err(error::Error::Other("pull stream died"))
        } else {
            Ok(())
        }
    }
}

impl ServerStreamHandler for StreamQueueSyncSender<ServerTypes> {
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

impl ClientStreamHandler for StreamQueueSyncSender<ClientTypes> {
    fn request_created(&mut self, _req: ClientRequest, _resp: Response) -> result::Result<()> {
        panic!("TODO")
    }

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

impl<T: Types> Stream for StreamQueueSyncReceiver<T> {
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

pub(crate) fn stream_queue_sync<T: Types>(
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
) -> (StreamQueueSyncSender<T>, StreamQueueSyncReceiver<T>) {
    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender {
        sender: utx,
        _marker: marker::PhantomData,
    };
    let rx = StreamQueueSyncReceiver {
        conn_died_error_holder,
        receiver: urx,
        eof_received: false,
        _marker: marker::PhantomData,
    };

    (tx, rx)
}
