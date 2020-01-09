//! Window size related types and constants

use std::fmt;

/// A sender MUST NOT allow a flow-control window to exceed 231-1 octets. If a sender receives
/// a WINDOW_UPDATE that causes a flow-control window to exceed this maximum,
/// it MUST terminate either the stream or the connection, as appropriate. For streams,
/// the sender sends a RST_STREAM with an error code of FLOW_CONTROL_ERROR; for the connection,
/// a GOAWAY frame with an error code of FLOW_CONTROL_ERROR is sent.
pub const MAX_WINDOW_SIZE: u32 = 0x7fffffff;

// TODO: MIN_WINDOW_SIZE

#[test]
fn test_max_window_size_is_i32_max() {
    use std::i32;
    assert_eq!(i32::max_value(), MAX_WINDOW_SIZE as i32);
}

// 6.9 WINDOW_UPDATE
/// The payload of a WINDOW_UPDATE frame is one reserved bit plus an unsigned 31-bit integer
/// indicating the number of octets that the sender can transmit in addition to the existing
/// flow-control window. The legal range for the increment to the flow-control window
/// is 1 to 231-1 (2,147,483,647) octets.
pub const MAX_WINDOW_SIZE_INC: u32 = 0x7fffffff;

/// The struct represents the size of a flow control window.
///
/// It exposes methods that allow the manipulation of window sizes, such that they can never
/// overflow the spec-mandated upper bound.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WindowSize(i32);
impl WindowSize {
    /// Add or subtract window size, check for overflow
    pub fn try_add(&mut self, delta: i32) -> Result<(), ()> {
        self.0 = match self.0.checked_add(delta) {
            Some(r) => r,
            None => return Err(()),
        };
        Ok(())
    }

    /// Tries to increase the window size by the given delta. If the WindowSize would overflow the
    /// maximum allowed value (2^31 - 1), returns an error case. If the increase succeeds, returns
    /// `Ok`.
    pub fn try_increase(&mut self, delta: u32) -> Result<(), ()> {
        // Someone's provided a delta that would definitely overflow the window size.
        if delta > MAX_WINDOW_SIZE_INC || delta == 0 {
            return Err(());
        }

        self.try_add(delta as i32)
    }

    /// Tries to decrease the size of the window by the given delta.
    ///
    /// There are situations where the window size should legitimately be allowed to become
    /// negative, so the only situation where the result is an error is if the window size would
    /// underflow, as this would definitely cause the peers to lose sync.
    pub fn try_decrease(&mut self, delta: i32) -> Result<(), ()> {
        match self.0.checked_sub(delta) {
            Some(new) => {
                self.0 = new;
                Ok(())
            }
            None => Err(()),
        }
    }

    pub fn try_decrease_to_non_negative(&mut self, delta: i32) -> Result<(), ()> {
        match self.0.checked_sub(delta) {
            Some(new) if new >= 0 => {
                self.0 = new;
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Creates a new `WindowSize` with the given initial size.
    pub fn new(size: i32) -> WindowSize {
        WindowSize(size)
    }
    /// Returns the current size of the window.
    ///
    /// The size is actually allowed to become negative (for instance if the peer changes its
    /// intial window size in the settings); therefore, the return is an `i32`.
    pub fn size(&self) -> i32 {
        self.0
    }

    /// Window size when it's know to be non-negative
    ///
    /// Panics if windows size if negative
    pub fn unsigned(&self) -> u32 {
        let r = self.0 as u32;
        assert_eq!(r as i32, self.0);
        r
    }
}

impl fmt::Display for WindowSize {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// In window size cannot be negative.
pub(crate) struct NonNegativeWindowSize(WindowSize);

impl NonNegativeWindowSize {
    pub fn new(size: i32) -> NonNegativeWindowSize {
        assert!(size >= 0);
        NonNegativeWindowSize(WindowSize::new(size))
    }

    pub fn size(&self) -> i32 {
        self.0.size()
    }

    pub fn try_decrease_to_non_negative(&mut self, delta: i32) -> Result<(), ()> {
        self.0.try_decrease_to_non_negative(delta)
    }

    pub fn try_increase(&mut self, delta: u32) -> Result<(), ()> {
        self.0.try_increase(delta)
    }
}
