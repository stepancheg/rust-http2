use common::increase_in_window::IncreaseInWindow;
use result;
use server::types::ServerTypes;

pub struct ServerIncreaseInWindow(pub(crate) IncreaseInWindow<ServerTypes>);

impl ServerIncreaseInWindow {
    pub fn in_window_size(&self) -> u32 {
        self.0.in_window_size()
    }

    pub fn data_frame_processed(&mut self, size: u32) {
        self.0.data_frame_processed(size)
    }

    pub fn increase_window(&mut self, inc: u32) -> result::Result<()> {
        self.0.increase_window(inc)
    }
}
