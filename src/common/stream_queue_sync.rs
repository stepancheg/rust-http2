#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;
use futures::sync::mpsc::UnboundedReceiver;

use futures_misc::ResultOrEof;

use error;

use stream_part::*;
use client_died_error_holder::*;


struct Shared {
    data_size: AtomicUsize,
}

pub struct StreamQueueSyncSender {
    shared: Arc<Shared>,
    sender: UnboundedSender<ResultOrEof<HttpStreamPart, error::Error>>,
}

pub struct StreamQueueSyncReceiver {
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
    shared: Arc<Shared>,
    receiver: UnboundedReceiver<ResultOrEof<HttpStreamPart, error::Error>>,
}

impl StreamQueueSyncSender {
    pub fn send(&self, item: ResultOrEof<HttpStreamPart, error::Error>) -> Result<(), ()> {
        if let ResultOrEof::Item(ref part) = item {
            if let &HttpStreamPart { content: HttpStreamPartContent::Data(ref b), .. } = part {
                self.shared.data_size.fetch_add(b.len(), Ordering::SeqCst);
            }
        }

        self.sender.unbounded_send(item).map_err(|_| ())
    }

    pub fn send_part(&self, part: HttpStreamPart) -> Result<(), ()> {
        self.send(ResultOrEof::Item(part))
    }

    pub fn send_error(&self, e: error::Error) -> Result<(), ()> {
        self.send(ResultOrEof::Error(e))
    }

    pub fn send_eof(&self) -> Result<(), ()> {
        self.send(ResultOrEof::Eof)
    }
}

impl StreamQueueSyncReceiver {
    pub fn data_size(&self) -> u32 {
        self.shared.data_size.load(Ordering::SeqCst) as u32
    }
}

impl Stream for StreamQueueSyncReceiver {
    type Item = HttpStreamPart;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<HttpStreamPart>, error::Error> {
        let part = match self.receiver.poll() {
            Err(()) => unreachable!(),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(None)) => return Err(self.conn_died_error_holder.error()),
            Ok(Async::Ready(Some(ResultOrEof::Error(e)))) => return Err(e),
            Ok(Async::Ready(Some(ResultOrEof::Eof))) => return Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(ResultOrEof::Item(part)))) => part,
        };


        if let HttpStreamPart { content: HttpStreamPartContent::Data(ref b) , .. } = part {
            self.shared.data_size.fetch_sub(b.len(), Ordering::SeqCst);
        }

        Ok(Async::Ready(Some(part)))
    }
}

pub fn stream_queue_sync(conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>)
    -> (StreamQueueSyncSender, StreamQueueSyncReceiver)
{
    let shared = Arc::new(Shared {
        data_size: AtomicUsize::new(0)
    });

    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender {
        shared: shared.clone(),
        sender: utx,
    };
    let rx = StreamQueueSyncReceiver {
        conn_died_error_holder,
        shared,
        receiver: urx,
    };

    (tx, rx)
}
