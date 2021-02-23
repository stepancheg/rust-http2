use crate::client_died_error_holder::DiedType;
use crate::client_died_error_holder::SomethingDiedErrorHolder;
use futures::channel::mpsc;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

pub(crate) trait ErrorAwareDrop {
    type DiedType: DiedType;

    fn drop_with_error(self, error: crate::Error);
}

pub(crate) struct DeathAwareSender<T: ErrorAwareDrop> {
    tx: UnboundedSender<T>,
    conn_died_error_holder: SomethingDiedErrorHolder<T::DiedType>,
}

pub(crate) struct DeathAwareReceiver<T: ErrorAwareDrop> {
    rx: UnboundedReceiver<T>,
    conn_died_error_holder: SomethingDiedErrorHolder<T::DiedType>,
}

impl<T: ErrorAwareDrop> Drop for DeathAwareReceiver<T> {
    fn drop(&mut self) {
        while let Ok(Some(m)) = self.rx.try_next() {
            m.drop_with_error(self.conn_died_error_holder.error());
        }
    }
}

impl<T: ErrorAwareDrop> Clone for DeathAwareSender<T> {
    fn clone(&self) -> Self {
        DeathAwareSender {
            tx: self.tx.clone(),
            conn_died_error_holder: self.conn_died_error_holder.clone(),
        }
    }
}

impl<T: ErrorAwareDrop> DeathAwareSender<T> {
    pub fn unbounded_send_recover(&self, msg: T) -> Result<(), (T, crate::Error)> {
        self.tx
            .unbounded_send(msg)
            .map_err(|e| (e.into_inner(), self.conn_died_error_holder.error()))
    }

    pub fn unbounded_send(&self, msg: T) -> crate::Result<()> {
        self.unbounded_send_recover(msg).map_err(|(_, e)| e)
    }
}

impl<T: ErrorAwareDrop> Stream for DeathAwareReceiver<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

pub(crate) fn death_aware_channel<T: ErrorAwareDrop>(
    conn_died_error_holder: SomethingDiedErrorHolder<T::DiedType>,
) -> (DeathAwareSender<T>, DeathAwareReceiver<T>) {
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
