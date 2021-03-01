use crate::death::error_holder::DiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use futures::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct DeathAwareOneshotNoContentDropSender<T, D: DiedType> {
    tx: oneshot::Sender<T>,
    died_error_holder: SomethingDiedErrorHolder<D>,
}

impl<T, D: DiedType> DeathAwareOneshotNoContentDropSender<T, D> {
    pub fn send(self, message: T) -> crate::Result<()> {
        match self.tx.send(message) {
            Ok(()) => Ok(()),
            Err(_) => Err(self.died_error_holder.error()),
        }
    }
}

pub(crate) struct DeathAwareOneshotNoContentDropReceiver<T, D: DiedType> {
    rx: oneshot::Receiver<T>,
    died_error_holder: SomethingDiedErrorHolder<D>,
}

impl<T, D: DiedType> Future for DeathAwareOneshotNoContentDropReceiver<T, D> {
    type Output = crate::Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<T>> {
        let self_mut = self.get_mut();
        match Pin::new(&mut self_mut.rx).poll(cx) {
            Poll::Ready(Ok(r)) => Poll::Ready(Ok(r)),
            Poll::Ready(Err(_)) => Poll::Ready(Err(self_mut.died_error_holder.error())),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) fn death_aware_oneshot_no_content_drop<T, D: DiedType>(
    died_error_holder: SomethingDiedErrorHolder<D>,
) -> (
    DeathAwareOneshotNoContentDropSender<T, D>,
    DeathAwareOneshotNoContentDropReceiver<T, D>,
) {
    let (tx, rx) = oneshot::channel();
    (
        DeathAwareOneshotNoContentDropSender {
            tx,
            died_error_holder: died_error_holder.clone(),
        },
        DeathAwareOneshotNoContentDropReceiver {
            rx,
            died_error_holder,
        },
    )
}
