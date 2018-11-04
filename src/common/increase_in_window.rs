use common::conn_command_channel::ConnCommandSender;
use common::conn_write::CommonToWriteMessage;
use common::types::Types;
use result;
use solicit::stream_id::StreamId;

pub(crate) struct IncreaseInWindow<T: Types> {
    pub stream_id: StreamId,
    pub to_write_tx: ConnCommandSender<T>,
}

impl<T: Types> IncreaseInWindow<T> {
    pub fn increase_window(&self, inc: u32) -> result::Result<()> {
        let m = CommonToWriteMessage::IncreaseInWindow(self.stream_id, inc);
        self.to_write_tx.unbounded_send(m.into())
    }
}
