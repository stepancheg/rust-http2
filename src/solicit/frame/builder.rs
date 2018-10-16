//! Defines the `FrameBuilder` trait and some default implementations of the trait.

use solicit::frame::pack_header;
use solicit::frame::FrameHeader;
use solicit::frame::FRAME_HEADER_LEN;

/// A trait that provides additional methods for serializing HTTP/2 frames.
///
/// All methods have a default implementation in terms of the `io::Write` API, but types can
/// provide specialized more efficient implementations if possible. This is effectively a
/// workaround for specialization not existing yet in Rust.
pub struct FrameBuilder(pub Vec<u8>);

impl FrameBuilder {
    pub fn new() -> FrameBuilder {
        FrameBuilder(Vec::new())
    }

    /// Write the given frame header as the next octets (i.e. without moving the cursor to the
    /// beginning of the buffer).
    pub fn write_header(&mut self, header: FrameHeader) {
        self.0.reserve(
            FRAME_HEADER_LEN
                .checked_add(header.length as usize)
                .expect("overflow"),
        );
        self.write_all(&pack_header(&header));
    }

    pub fn write_all(&mut self, buf: &[u8]) {
        self.0.extend(buf);
    }

    /// Write the given number of padding octets.
    ///
    /// The default implementation invokes the underlying Writer's `write` method `padding_length`
    /// times.
    ///
    /// Other `FrameBuilder` implementations could implement it more efficiently (e.g. if it is
    /// known that the `FrameBuilder` is backed by a zeroed buffer, there's no need to write
    /// anything, only increment a cursor/offset).
    pub fn write_padding(&mut self, padding_length: u8) {
        self.0.extend((0..padding_length).map(|_| 0))
    }

    /// Write the given unsigned 32 bit integer to the underlying stream. The integer is written as
    /// four bytes in network endian style.
    pub fn write_u32(&mut self, num: u32) {
        self.write_all(&[
            (((num >> 24) & 0x000000FF) as u8),
            (((num >> 16) & 0x000000FF) as u8),
            (((num >> 8) & 0x000000FF) as u8),
            (((num) & 0x000000FF) as u8),
        ])
    }
}
