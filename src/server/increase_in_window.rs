use common::increase_in_window::IncreaseInWindow;
use result;
use server::types::ServerTypes;

pub struct ServerIncreaseInWindow(pub(crate) IncreaseInWindow<ServerTypes>);

impl ServerIncreaseInWindow {
    pub fn increase_window(&self, inc: u32) -> result::Result<()> {
        self.0.increase_window(inc)
    }
}
