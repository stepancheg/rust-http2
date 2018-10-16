//! The module contains some common utilities for `solicit::http` tests.

use std::io::Write;

use solicit::frame::{pack_header, FrameHeader, RawFrame};

/// Creates a new `RawFrame` from two separate parts: the header and the payload.
/// Useful for tests that need to create frames, since they can easily specify the header and the
/// payload separately and use this function to stitch them together into a `RawFrame`.
#[cfg(test)]
pub fn raw_frame_from_parts(header: FrameHeader, payload: Vec<u8>) -> RawFrame {
    let mut buf = Vec::new();
    assert_eq!(9, buf.write(&pack_header(&header)[..]).unwrap());
    assert_eq!(payload.len(), buf.write(&payload).unwrap());
    buf.into()
}
