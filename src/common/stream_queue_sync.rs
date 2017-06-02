#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;
use futures::sync::mpsc::UnboundedReceiver;

use stream_part::*;


struct StreamQueueSyncShared {
    data_size: AtomicUsize,
}

impl StreamQueueSyncShared {
    pub fn data_size(&self) -> usize {
        self.data_size.load(Ordering::SeqCst)
    }
}

pub struct StreamQueueSyncSender {
    shared: Arc<StreamQueueSyncShared>,
    sender: UnboundedSender<HttpStreamPart>,
}

pub struct StreamQueueSyncReceiver {
    shared: Arc<StreamQueueSyncShared>,
    receiver: UnboundedReceiver<HttpStreamPart>,
}

impl StreamQueueSyncSender {
    pub fn send(&self, _part: HttpStreamPart) {
        unimplemented!()
    }
}

impl StreamQueueSyncReceiver {

}

pub fn stream_queue_sync() -> (StreamQueueSyncSender, StreamQueueSyncReceiver) {
    let shared = Arc::new(StreamQueueSyncShared {
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
