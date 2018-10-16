use futures::future::Future;
use futures::stream::Stream;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
use futures::Async;
use futures::Poll;

use void::Void;

pub fn shutdown_signal() -> (ShutdownSignal, ShutdownFuture) {
    let (tx, rx) = unbounded();
    (ShutdownSignal { tx: tx }, ShutdownFuture { rx: rx })
}

pub struct ShutdownSignal {
    tx: UnboundedSender<()>,
}

impl ShutdownSignal {
    pub fn shutdown(&self) {
        // ignore error, because receiver may be already removed
        drop(self.tx.unbounded_send(()));
    }
}

impl Drop for ShutdownSignal {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct ShutdownFuture {
    rx: UnboundedReceiver<()>,
}

impl Future for ShutdownFuture {
    type Item = Void;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.rx.poll() {
            Ok(Async::Ready(_)) => Err(()),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(_) => Err(()),
        }
    }
}
