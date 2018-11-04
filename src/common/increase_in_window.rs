use common::conn_write::CommonToWriteMessage;
use common::types::Types;
use error;
use futures::sync::mpsc::UnboundedSender;
use result;
use solicit::stream_id::StreamId;

pub(crate) struct IncreaseInWindow<T: Types> {
    pub stream_id: StreamId,
    pub to_write_tx: UnboundedSender<T::ToWriteMessage>,
}

impl<T: Types> IncreaseInWindow<T> {
    pub fn increase_window(&self, inc: u32) -> result::Result<()> {
        let m = CommonToWriteMessage::IncreaseInWindow(self.stream_id, inc);
        if let Err(_) = self.to_write_tx.unbounded_send(m.into()) {
            // TODO: better message
            return Err(error::Error::Other("failed to send to conn; likely died"));
        }
        Ok(())
    }
}
