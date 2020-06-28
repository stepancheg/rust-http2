use crate::client_died_error_holder::ConnDiedType;
use crate::client_died_error_holder::SomethingDiedErrorHolder;
use crate::common::types::Types;
use crate::result;
use futures::channel::mpsc;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

pub(crate) struct ConnCommandSender<T: Types> {
    tx: UnboundedSender<T::ToWriteMessage>,
    conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,
}

impl<T: Types> Clone for ConnCommandSender<T> {
    fn clone(&self) -> Self {
        ConnCommandSender {
            tx: self.tx.clone(),
            conn_died_error_holder: self.conn_died_error_holder.clone(),
        }
    }
}

impl<T: Types> ConnCommandSender<T> {
    pub fn unbounded_send_recover(&self, msg: T::ToWriteMessage) -> Result<(), T::ToWriteMessage> {
        self.tx.unbounded_send(msg).map_err(|e| e.into_inner())
    }

    pub fn unbounded_send(&self, msg: T::ToWriteMessage) -> result::Result<()> {
        match self.tx.unbounded_send(msg) {
            Ok(()) => Ok(()),
            Err(_) => Err(self.conn_died_error_holder.error()),
        }
    }
}

pub(crate) struct ConnCommandReceiver<T: Types> {
    rx: UnboundedReceiver<T::ToWriteMessage>,
}

impl<T: Types> Stream for ConnCommandReceiver<T> {
    type Item = T::ToWriteMessage;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<T::ToWriteMessage>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

pub(crate) fn conn_command_channel<T: Types>(
    conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,
) -> (ConnCommandSender<T>, ConnCommandReceiver<T>) {
    let (tx, rx) = mpsc::unbounded();
    let tx = ConnCommandSender {
        tx,
        conn_died_error_holder,
    };
    let rx = ConnCommandReceiver { rx };
    (tx, rx)
}
