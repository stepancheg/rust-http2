use futures::channel::mpsc::unbounded;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;
use std::future::Future;
use std::task::Poll;

use futures::task::Context;
use std::pin::Pin;
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
    type Output = Result<Void, ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Ready(_) => Poll::Ready(Err(())),
            Poll::Pending => Poll::Pending,
        }
    }
}
