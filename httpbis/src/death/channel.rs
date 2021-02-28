use crate::death::error_holder::DiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use futures::channel::mpsc;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

pub(crate) trait ErrorAwareDrop {
    fn drop_with_error(self, error: crate::Error);
}

pub(crate) struct DeathAwareSender<T: ErrorAwareDrop, D: DiedType> {
    tx: UnboundedSender<T>,
    conn_died_error_holder: SomethingDiedErrorHolder<D>,
}

pub(crate) struct DeathAwareReceiver<T: ErrorAwareDrop, D: DiedType> {
    rx: UnboundedReceiver<T>,
    conn_died_error_holder: SomethingDiedErrorHolder<D>,
}

impl<T: ErrorAwareDrop, D: DiedType> Drop for DeathAwareReceiver<T, D> {
    fn drop(&mut self) {
        self.rx.close();

        while let Ok(Some(m)) = self.rx.try_next() {
            m.drop_with_error(self.conn_died_error_holder.error());
        }
    }
}

impl<T: ErrorAwareDrop, D: DiedType> Clone for DeathAwareSender<T, D> {
    fn clone(&self) -> Self {
        DeathAwareSender {
            tx: self.tx.clone(),
            conn_died_error_holder: self.conn_died_error_holder.clone(),
        }
    }
}

impl<T: ErrorAwareDrop, D: DiedType> DeathAwareSender<T, D> {
    pub fn unbounded_send_recover(&self, msg: T) -> Result<(), (T, crate::Error)> {
        self.tx
            .unbounded_send(msg)
            .map_err(|e| (e.into_inner(), self.conn_died_error_holder.error()))
    }

    pub fn unbounded_send(&self, msg: T) -> crate::Result<()> {
        self.unbounded_send_recover(msg).map_err(|(_, e)| e)
    }

    /// Send a message, but call `ErrorAwareDrop` on error.
    pub fn unbounded_send_no_result(&self, msg: T) {
        match self.unbounded_send_recover(msg) {
            Ok(()) => {}
            Err((msg, e)) => msg.drop_with_error(e),
        }
    }
}

impl<T: ErrorAwareDrop, D: DiedType> Stream for DeathAwareReceiver<T, D> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

pub(crate) fn death_aware_channel<T: ErrorAwareDrop, D: DiedType>(
    conn_died_error_holder: SomethingDiedErrorHolder<D>,
) -> (DeathAwareSender<T, D>, DeathAwareReceiver<T, D>) {
    let (tx, rx) = mpsc::unbounded();
    let tx = DeathAwareSender {
        tx,
        conn_died_error_holder: conn_died_error_holder.clone(),
    };
    let rx = DeathAwareReceiver {
        rx,
        conn_died_error_holder,
    };
    (tx, rx)
}
