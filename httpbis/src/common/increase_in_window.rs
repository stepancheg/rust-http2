use crate::client::types::ClientTypes;
use crate::common::increase_in_window_common::IncreaseInWindowCommon;
use crate::server::types::ServerTypes;

enum Impl {
    Client(IncreaseInWindowCommon<ClientTypes>),
    Server(IncreaseInWindowCommon<ServerTypes>),
}

/// Utility to tell the peer that it can send more data.
pub struct IncreaseInWindow(Impl);

impl From<IncreaseInWindowCommon<ClientTypes>> for IncreaseInWindow {
    fn from(c: IncreaseInWindowCommon<ClientTypes>) -> Self {
        IncreaseInWindow(Impl::Client(c))
    }
}

impl From<IncreaseInWindowCommon<ServerTypes>> for IncreaseInWindow {
    fn from(c: IncreaseInWindowCommon<ServerTypes>) -> Self {
        IncreaseInWindow(Impl::Server(c))
    }
}

impl IncreaseInWindow {
    /// Current incoming window size.
    pub fn in_window_size(&self) -> u32 {
        match &self.0 {
            Impl::Client(c) => c.in_window_size(),
            Impl::Server(c) => c.in_window_size(),
        }
    }

    /// Decrement window size when new data frame recevied.
    pub fn data_frame_processed(&mut self, size: usize) {
        match &mut self.0 {
            Impl::Client(c) => c.data_frame_received(size),
            Impl::Server(c) => c.data_frame_received(size),
        }
    }

    /// Notify peer to increase in window.
    pub fn increase_window(&mut self, inc: u32) -> crate::Result<()> {
        match &mut self.0 {
            Impl::Client(c) => c.increase_window(inc),
            Impl::Server(c) => c.increase_window(inc),
        }
    }

    /// Auto-increase in window.
    // TODO: drop this operation.
    pub fn increase_window_auto(&mut self) -> crate::Result<()> {
        match &mut self.0 {
            Impl::Client(c) => c.increase_window_auto(),
            Impl::Server(c) => c.increase_window_auto(),
        }
    }

    /// Auto-increase in window above specified limit.
    // TODO: drop this operation.
    pub fn increase_window_auto_above(&mut self, above: u32) -> crate::Result<()> {
        match &mut self.0 {
            Impl::Client(c) => c.increase_window_auto_above(above),
            Impl::Server(c) => c.increase_window_auto_above(above),
        }
    }
}
