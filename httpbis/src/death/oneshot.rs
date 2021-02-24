use crate::death::channel::ErrorAwareDrop;
use crate::death::error_holder::DiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use futures::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::sync::oneshot;

pub(crate) struct DeathAwareOneshotSender<T: ErrorAwareDrop, D: DiedType> {
    tx: oneshot::Sender<T>,
    died_error_holder: SomethingDiedErrorHolder<D>,
}

impl<T: ErrorAwareDrop, D: DiedType> DeathAwareOneshotSender<T, D> {
    pub fn send(self, message: T) {
        match self.tx.send(message) {
            Ok(()) => {}
            Err(message) => message.drop_with_error(self.died_error_holder.error()),
        }
    }
}

pub(crate) struct DeathAwareOneshotReceiver<T: ErrorAwareDrop, D: DiedType> {
    rx: oneshot::Receiver<T>,
    received: bool, // https://github.com/tokio-rs/tokio/pull/3552
    died_error_holder: SomethingDiedErrorHolder<D>,
}

impl<T: ErrorAwareDrop, D: DiedType> Future for DeathAwareOneshotReceiver<T, D> {
    type Output = crate::Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<T>> {
        let self_mut = self.get_mut();
        match Pin::new(&mut self_mut.rx).poll(cx) {
            Poll::Ready(Ok(r)) => {
                self_mut.received = true;
                Poll::Ready(Ok(r))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(self_mut.died_error_holder.error())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: ErrorAwareDrop, D: DiedType> Drop for DeathAwareOneshotReceiver<T, D> {
    fn drop(&mut self) {
        if !(self.received) {
            self.rx.close();
            if let Ok(m) = self.rx.try_recv() {
                m.drop_with_error(self.died_error_holder.error());
            }
        }
    }
}

pub(crate) fn death_aware_oneshot<T: ErrorAwareDrop, D: DiedType>(
    died_error_holder: SomethingDiedErrorHolder<D>,
) -> (
    DeathAwareOneshotSender<T, D>,
    DeathAwareOneshotReceiver<T, D>,
) {
    let (tx, rx) = oneshot::channel();
    (
        DeathAwareOneshotSender {
            tx,
            died_error_holder: died_error_holder.clone(),
        },
        DeathAwareOneshotReceiver {
            rx,
            received: false,
            died_error_holder,
        },
    )
}
