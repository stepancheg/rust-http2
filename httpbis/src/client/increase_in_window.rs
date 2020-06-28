use crate::client::types::ClientTypes;
use crate::common::increase_in_window::IncreaseInWindow;
use crate::result;

pub struct ClientIncreaseInWindow(pub(crate) IncreaseInWindow<ClientTypes>);

impl ClientIncreaseInWindow {
    pub fn in_window_size(&self) -> u32 {
        self.0.in_window_size()
    }

    pub fn data_frame_processed(&mut self, size: u32) {
        self.0.data_frame_processed(size)
    }

    pub fn increase_window(&mut self, inc: u32) -> result::Result<()> {
        self.0.increase_window(inc)
    }

    pub fn increase_window_auto(&mut self) -> result::Result<()> {
        self.0.increase_window_auto()
    }

    pub fn increase_window_auto_above(&mut self, above: u32) -> result::Result<()> {
        self.0.increase_window_auto_above(above)
    }
}
