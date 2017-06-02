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


struct Shared {
    data_size: AtomicUsize,
}

impl Shared {
    pub fn data_size(&self) -> usize {
        self.data_size.load(Ordering::SeqCst)
    }
}

pub struct StreamQueueSyncSender {
    shared: Arc<Shared>,
    sender: UnboundedSender<ResultOrEof<HttpStreamPart, error::Error>>,
}

pub struct StreamQueueSyncReceiver {
    shared: Arc<Shared>,
    receiver: UnboundedReceiver<ResultOrEof<HttpStreamPart, error::Error>>,
}

impl StreamQueueSyncSender {
    pub fn send(&self, part: HttpStreamPart) -> Result<(), ()> {
        if let HttpStreamPartContent::Data(ref b) = part.content {
            self.shared.data_size.fetch_add(b.len(), Ordering::SeqCst);
        }
        self.sender.send(ResultOrEof::Item(part)).map_err(|_| ())
    }

    pub fn send_error(&self, e: error::Error) -> Result<(), ()> {
        self.sender.send(ResultOrEof::Error(e)).map_err(|_| ())
    }

    pub fn send_eof(&self) -> Result<(), ()> {
        self.sender.send(ResultOrEof::Eof).map_err(|_| ())
    }
}

impl Stream for StreamQueueSyncReceiver {
    type Item = HttpStreamPart;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<HttpStreamPart>, error::Error> {
        let part = match self.receiver.poll() {
            Err(()) => unreachable!(),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(None)) => return Err(error::Error::Other("unexpected EOF; conn likely died")),
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

pub fn stream_queue_sync() -> (StreamQueueSyncSender, StreamQueueSyncReceiver) {
    let shared = Arc::new(Shared {
        data_size: AtomicUsize::new(0)
    });

    let (utx, urx) = unbounded();

    let tx = StreamQueueSyncSender {
        shared: shared.clone(),
        sender: utx,
    };
    let rx = StreamQueueSyncReceiver {
        shared: shared,
        receiver: urx,
    };

    (tx, rx)
}
