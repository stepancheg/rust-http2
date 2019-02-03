use common::conn_command_channel::ConnCommandSender;
use common::conn_write::CommonToWriteMessage;
use common::types::Types;
use result;
use solicit::stream_id::StreamId;
use solicit::DEFAULT_SETTINGS;

pub(crate) struct IncreaseInWindow<T: Types> {
    pub stream_id: StreamId,
    pub in_window_size: u32,
    pub to_write_tx: ConnCommandSender<T>,
}

impl<T: Types> IncreaseInWindow<T> {
    /// Currently known window size.
    /// Valid only if properly updated by `data_frame_received`
    pub fn in_window_size(&self) -> u32 {
        self.in_window_size
    }

    /// Decrement window size when new data frame recevied.
    pub fn data_frame_processed(&mut self, size: u32) {
        self.in_window_size = self.in_window_size.checked_sub(size).unwrap();
    }

    pub fn increase_window(&mut self, inc: u32) -> result::Result<()> {
        self.in_window_size = self.in_window_size.checked_add(inc).unwrap();
        let m = CommonToWriteMessage::IncreaseInWindow(self.stream_id, inc);
        self.to_write_tx.unbounded_send(m.into())
    }

    pub fn increase_window_auto(&mut self) -> result::Result<()> {
        if self.in_window_size < DEFAULT_SETTINGS.initial_window_size / 2 {
            self.increase_window(DEFAULT_SETTINGS.initial_window_size)
        } else {
            Ok(())
        }
    }
}

impl<T: Types> Drop for IncreaseInWindow<T> {
    fn drop(&mut self) {
        // TODO: cancel the stream
    }
}
