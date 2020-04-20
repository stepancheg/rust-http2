//! Defines the `FrameBuilder` trait and some default implementations of the trait.

use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::frame::pack_header;
use crate::solicit::frame::FrameHeader;
use crate::solicit::frame::FRAME_HEADER_LEN;

/// A trait that provides additional methods for serializing HTTP/2 frames.
pub trait FrameBuilder {
    fn reserve(&mut self, _additional: usize) {}

    fn write_slice(&mut self, bytes: &[u8]);

    /// Write the given frame header as the next octets (i.e. without moving the cursor to the
    /// beginning of the buffer).
    fn write_header(&mut self, header: FrameHeader) {
        self.reserve(
            FRAME_HEADER_LEN
                .checked_add(header.payload_len as usize)
                .expect("overflow"),
        );
        self.write_slice(&pack_header(&header));
    }

    /// Write the given number of padding octets.
    ///
    /// The default implementation invokes the underlying Writer's `write` method `padding_length`
    /// times.
    ///
    /// Other `FrameBuilder` implementations could implement it more efficiently (e.g. if it is
    /// known that the `FrameBuilder` is backed by a zeroed buffer, there's no need to write
    /// anything, only increment a cursor/offset).
    fn write_padding(&mut self, padding_length: u8);

    /// Write the given unsigned 32 bit integer to the underlying stream. The integer is written as
    /// four bytes in network endian style.
    fn write_u32(&mut self, num: u32) {
        self.write_slice(&[
            (((num >> 24) & 0x000000FF) as u8),
            (((num >> 16) & 0x000000FF) as u8),
            (((num >> 8) & 0x000000FF) as u8),
            (((num) & 0x000000FF) as u8),
        ])
    }
}

impl FrameBuilder for WriteBuffer {
    fn write_slice(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }

    fn write_header(&mut self, header: FrameHeader) {
        self.extend_frame_header_buffer(pack_header(&header));
    }

    fn write_padding(&mut self, padding_length: u8) {
        self.extend_with_zeroes(padding_length as usize);
    }
}
