#![allow(dead_code)]

use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
use futures::Async;
use futures::Poll;

use result_or_eof::ResultOrEof;

use error;

use client_died_error_holder::*;
use data_or_headers_with_flag::DataOrHeadersWithFlag;

pub struct StreamQueueSyncSender {
    sender: UnboundedSender<ResultOrEof<DataOrHeadersWithFlag, error::Error>>,
}

pub struct StreamQueueSyncReceiver {
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
    receiver: UnboundedReceiver<ResultOrEof<DataOrHeadersWithFlag, error::Error>>,
}

impl StreamQueueSyncSender {
    pub fn send(&self, item: ResultOrEof<DataOrHeadersWithFlag, error::Error>) -> Result<(), ()> {
        self.sender.unbounded_send(item).map_err(|_| ())
    }

    pub fn send_part(&self, part: DataOrHeadersWithFlag) -> Result<(), ()> {
        self.send(ResultOrEof::Item(part))
    }

    pub fn send_error(&self, e: error::Error) -> Result<(), ()> {
        self.send(ResultOrEof::Error(e))
    }

    pub fn send_eof(&self) -> Result<(), ()> {
        self.send(ResultOrEof::Eof)
    }
}

impl StreamQueueSyncReceiver {}

impl Stream for StreamQueueSyncReceiver {
    type Item = DataOrHeadersWithFlag;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<DataOrHeadersWithFlag>, error::Error> {
        let part = match self.receiver.poll() {
            Err(()) => unreachable!(),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(None)) => return Err(self.conn_died_error_holder.error()),
            Ok(Async::Ready(Some(ResultOrEof::Error(e)))) => return Err(e),
            Ok(Async::Ready(Some(ResultOrEof::Eof))) => return Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(ResultOrEof::Item(part)))) => part,
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
    };

    (tx, rx)
}
