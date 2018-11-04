use client_died_error_holder::ClientConnDiedType;
use client_died_error_holder::ClientDiedErrorHolder;
use common::types::Types;
use futures::sync::mpsc;
use futures::sync::mpsc::UnboundedReceiver;
use futures::sync::mpsc::UnboundedSender;
use futures::Poll;
use futures::Stream;
use result;
use void::Void;

pub(crate) struct ConnCommandSender<T: Types> {
    tx: UnboundedSender<T::ToWriteMessage>,
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
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
    type Error = Void;

    fn poll(&mut self) -> Poll<Option<T::ToWriteMessage>, Void> {
        self.rx.poll().map_err(|()| unreachable!())
    }
}

pub(crate) fn conn_command_channel<T: Types>(
    conn_died_error_holder: ClientDiedErrorHolder<ClientConnDiedType>,
) -> (ConnCommandSender<T>, ConnCommandReceiver<T>) {
    let (tx, rx) = mpsc::unbounded();
    let tx = ConnCommandSender {
        tx,
        conn_died_error_holder,
    };
    let rx = ConnCommandReceiver { rx };
    (tx, rx)
}
