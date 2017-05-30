//! Defines the `FrameBuilder` trait and some default implementations of the trait.

use std::io;
use solicit::frame::{FrameHeader, pack_header};

/// A trait that provides additional methods for serializing HTTP/2 frames.
///
/// All methods have a default implementation in terms of the `io::Write` API, but types can
/// provide specialized more efficient implementations if possible. This is effectively a
/// workaround for specialization not existing yet in Rust.
pub trait FrameBuilder: io::Write + io::Seek {
    /// Write the given frame header as the next octets (i.e. without moving the cursor to the
    /// beginning of the buffer).
    fn write_header(&mut self, header: FrameHeader) -> io::Result<()> {
        self.write_all(&pack_header(&header))
    }

    /// Write the given number of padding octets.
    ///
    /// The default implementation invokes the underlying Writer's `write` method `padding_length`
    /// times.
    ///
    /// Other `FrameBuilder` implementations could implement it more efficiently (e.g. if it is
    /// known that the `FrameBuilder` is backed by a zeroed buffer, there's no need to write
    /// anything, only increment a cursor/offset).
    fn write_padding(&mut self, padding_length: u8) -> io::Result<()> {
        for _ in 0..padding_length {
            self.write_all(&[0])?;
        }
        Ok(())
    }

    /// Write the given unsigned 32 bit integer to the underlying stream. The integer is written as
    /// four bytes in network endian style.
    fn write_u32(&mut self, num: u32) -> io::Result<()> {
        self.write_all(&[(((num >> 24) & 0x000000FF) as u8),
                         (((num >> 16) & 0x000000FF) as u8),
                         (((num >>  8) & 0x000000FF) as u8),
                         (((num      ) & 0x000000FF) as u8)])
    }
}

impl FrameBuilder for io::Cursor<Vec<u8>> {}

#[cfg(test)]
mod tests {
    use super::FrameBuilder;
    use std::io::{self, Write};

    use solicit::frame::pack_header;
    use solicit::frame::FrameHeader;

    #[test]
    fn test_write_header_empty_buffer() {
        let mut buf = io::Cursor::new(Vec::new());
        let header = FrameHeader::new(10, 0x1, 0x0, 3);
        let expected = pack_header(&header);

        buf.write_header(header).unwrap();

        let frame = buf.into_inner();
        assert_eq!(frame, expected);
    }

    #[test]
    fn test_write_header_and_payload_empty() {
        let mut buf = io::Cursor::new(Vec::new());
        let header = FrameHeader::new(10, 0x1, 0x0, 3);
        let expected = {
            let mut buf = Vec::new();
            buf.extend(pack_header(&header).to_vec());
            buf.extend(vec![1, 2, 3, 4]);
            buf
        };

        buf.write_header(header).unwrap();
        buf.write_all(&[1, 2, 3, 4]).unwrap();

        let frame = buf.into_inner();
        assert_eq!(frame, expected);
    }

    #[test]
    fn test_write_padding() {
        let mut buf = io::Cursor::new(Vec::new());
        buf.write_padding(5).unwrap();

        assert_eq!(buf.into_inner(), vec![0; 5]);
    }

    #[test]
    fn test_write_u32() {
        fn get_written(num: u32) -> Vec<u8> {
            let mut buf = io::Cursor::new(Vec::new());
            buf.write_u32(num).unwrap();
            buf.into_inner()
        }

        assert_eq!(get_written(0x0), vec![0, 0, 0, 0]);
        assert_eq!(get_written(0x1), vec![0, 0, 0, 1]);
        assert_eq!(get_written(0x10), vec![0, 0, 0, 0x10]);
        assert_eq!(get_written(0x10AB00CC), vec![0x10, 0xAB, 0, 0xCC]);
        assert_eq!(get_written(0xFFFFFFFF), vec![0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(get_written(0xEFFFFFFF), vec![0xEF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(get_written(0x7FFFFFFF), vec![0x7F, 0xFF, 0xFF, 0xFF]);
        assert_eq!(get_written(0xFFFFFF7F), vec![0xFF, 0xFF, 0xFF, 0x7F]);
    }
}
