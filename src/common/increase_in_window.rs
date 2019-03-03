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
        let old_in_window_size = self.in_window_size;
        self.in_window_size = self.in_window_size.checked_sub(size).unwrap();
        debug!(
            "data frame processed, in window size: {} -> {}",
            old_in_window_size, self.in_window_size
        );
    }

    pub fn increase_window(&mut self, inc: u32) -> result::Result<()> {
        let old_in_window_size = self.in_window_size;
        // TODO: do not panic
        self.in_window_size = self.in_window_size.checked_add(inc).unwrap();
        debug!(
            "requesting increase stream window: {} -> {}",
            old_in_window_size, self.in_window_size
        );
        let m = CommonToWriteMessage::IncreaseInWindow(self.stream_id, inc);
        self.to_write_tx.unbounded_send(m.into())
    }

    pub fn increase_window_auto_above(&mut self, above: u32) -> result::Result<()> {
        // TODO: overflow check
        if self.in_window_size < above + DEFAULT_SETTINGS.initial_window_size / 2 {
            self.increase_window(DEFAULT_SETTINGS.initial_window_size)
        } else {
            Ok(())
        }
    }

    pub fn increase_window_auto(&mut self) -> result::Result<()> {
        self.increase_window_auto_above(0)
    }
}

impl<T: Types> Drop for IncreaseInWindow<T> {
    fn drop(&mut self) {
        // TODO: cancel the stream
    }
}
