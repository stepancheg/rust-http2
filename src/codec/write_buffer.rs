use crate::bytes_ext::buf_vec_deque::BufVecDeque;
use crate::codec::zeroes::Zeroes;
use crate::solicit::frame::FrameHeaderBuffer;
use crate::BufGetBytes;
use bytes::Buf;
use bytes::Bytes;
use std::io::Cursor;
use std::io::IoSlice;
use std::mem;

enum Item {
    Vec(Cursor<Vec<u8>>),
    Bytes(Bytes),
    FrameHeaderBuffer(Cursor<FrameHeaderBuffer>),
    Zeroes(Zeroes),
}

impl Buf for Item {
    fn remaining(&self) -> usize {
        match self {
            Item::Vec(v) => v.remaining(),
            Item::Bytes(b) => b.remaining(),
            Item::FrameHeaderBuffer(c) => c.remaining(),
            Item::Zeroes(z) => z.remaining(),
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Item::Vec(v) => v.bytes(),
            Item::Bytes(b) => b.bytes(),
            Item::FrameHeaderBuffer(c) => c.bytes(),
            Item::Zeroes(z) => z.bytes(),
        }
    }

    fn bytes_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        match self {
            Item::Vec(v) => v.bytes_vectored(dst),
            Item::Bytes(b) => b.bytes_vectored(dst),
            Item::FrameHeaderBuffer(c) => c.bytes_vectored(dst),
            Item::Zeroes(z) => z.bytes_vectored(dst),
        }
    }

    fn advance(&mut self, cnt: usize) {
        match self {
            Item::Vec(v) => v.advance(cnt),
            Item::Bytes(b) => b.advance(cnt),
            Item::FrameHeaderBuffer(v) => v.advance(cnt),
            Item::Zeroes(z) => z.advance(cnt),
        }
    }
}

impl BufGetBytes for Item {
    fn get_bytes(&mut self, cnt: usize) -> Bytes {
        match self {
            Item::Vec(v) => v.get_bytes(cnt),
            Item::Bytes(b) => b.get_bytes(cnt),
            Item::FrameHeaderBuffer(v) => v.get_bytes(cnt),
            Item::Zeroes(z) => z.get_bytes(cnt),
        }
    }
}

#[derive(Default)]
pub struct WriteBuffer {
    deque: BufVecDeque<Item>,
}

impl Buf for WriteBuffer {
    /// Size of data in the buffer
    fn remaining(&self) -> usize {
        self.deque.remaining()
    }

    fn bytes(&self) -> &[u8] {
        self.deque.bytes()
    }

    fn bytes_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        self.deque.bytes_vectored(dst)
    }

    fn advance(&mut self, cnt: usize) {
        self.deque.advance(cnt)
    }
}

impl WriteBuffer {
    pub fn new() -> WriteBuffer {
        Default::default()
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        self.tail_vec().extend_from_slice(data);
    }

    pub fn extend_from_bytes(&mut self, data: Bytes) {
        if data.is_empty() {
            return;
        }
        self.deque.push_back(Item::Bytes(data));
    }

    pub fn extend_frame_header_buffer(&mut self, buffer: FrameHeaderBuffer) {
        self.deque
            .push_back(Item::FrameHeaderBuffer(Cursor::new(buffer)));
    }

    pub fn extend_with_zeroes(&mut self, zeroes: usize) {
        if zeroes == 0 {
            return;
        }
        self.deque.push_back(Item::Zeroes(Zeroes(zeroes)));
    }

    pub fn tail_vec(&mut self) -> WriteBufferTailVec {
        match self.deque.pop_back() {
            Some(Item::Vec(cursor)) => WriteBufferTailVec {
                write_buffer: self,
                position: cursor.position() as usize,
                data: cursor.into_inner(),
            },
            o => {
                if let Some(v) = o {
                    self.deque.push_back(v);
                }
                WriteBufferTailVec {
                    write_buffer: self,
                    data: Vec::new(),
                    position: 0,
                }
            }
        }
    }
}

impl Into<Vec<u8>> for WriteBuffer {
    fn into(mut self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.remaining());
        while self.has_remaining() {
            let bytes = self.bytes();
            v.extend_from_slice(bytes);
            let len = bytes.len();
            self.advance(len);
        }
        v
    }
}

impl Into<Bytes> for WriteBuffer {
    fn into(self) -> Bytes {
        Bytes::from(Into::<Vec<u8>>::into(self))
    }
}

pub struct WriteBufferTailVec<'a> {
    write_buffer: &'a mut WriteBuffer,
    data: Vec<u8>,
    position: usize,
}

impl<'a> Drop for WriteBufferTailVec<'a> {
    fn drop(&mut self) {
        let mut cursor = Cursor::new(mem::take(&mut self.data));
        cursor.set_position(self.position as u64);
        self.write_buffer.deque.push_back(Item::Vec(cursor))
    }
}

impl<'a> WriteBufferTailVec<'a> {
    /// Size of data in the buffer
    pub fn remaining(&self) -> usize {
        debug_assert!(self.position <= self.data.len());
        self.data.len() - self.position
    }

    /// Pos is relative to "data"
    pub fn patch_buf(&mut self, pos: usize, data: &[u8]) {
        let patch_pos = self.position + pos;
        (&mut self.data[patch_pos..patch_pos + data.len()]).copy_from_slice(data);
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        // Could do something smarter
        self.reserve(data.len());
        self.data.extend_from_slice(data);
    }

    pub fn reserve(&mut self, additional: usize) {
        if self.remaining() >= additional {
            return;
        }
        self.compact();
        self.data.reserve(additional);
    }

    pub fn compact(&mut self) {
        self.data.drain(..self.position);
        self.position = 0;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn remaining() {
        let mut buf = WriteBuffer::new();
        buf.extend_from_slice(b"abcd");
        assert_eq!(4, buf.remaining());

        assert_eq!(b'a', buf.get_u8());
        assert_eq!(b'b', buf.get_u8());
        assert_eq!(2, buf.remaining());

        buf.extend_from_slice(b"ef");
        assert_eq!(b'c', buf.get_u8());
        assert_eq!(b'd', buf.get_u8());
        assert_eq!(b'e', buf.get_u8());
        assert_eq!(b'f', buf.get_u8());
        assert_eq!(0, buf.remaining());
    }
}
