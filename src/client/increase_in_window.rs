use client::types::ClientTypes;
use common::increase_in_window::IncreaseInWindow;
use result;

pub struct ClientIncreaseInWindow(pub(crate) IncreaseInWindow<ClientTypes>);

impl ClientIncreaseInWindow {
    pub fn increase_window(&self, inc: u32) -> result::Result<()> {
        self.0.increase_window(inc)
    }
}
