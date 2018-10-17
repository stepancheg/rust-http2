use bytes::Buf;
use bytes::Bytes;

// TODO: some tests
#[derive(Default)]
pub struct WriteBuffer {
    data: Vec<u8>,
    position: usize, // must be `<= data.len()`
}

impl Buf for WriteBuffer {
    fn remaining(&self) -> usize {
        debug_assert!(self.position <= self.data.len());
        self.data.len() - self.position
    }

    fn bytes(&self) -> &[u8] {
        &self.data[self.position..]
    }

    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.remaining());
        self.position += cnt;
    }
}

impl WriteBuffer {
    pub fn new() -> WriteBuffer {
        Default::default()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.data.reserve(additional)
    }

    pub fn compact(&mut self) {
        self.data.drain(..self.position);
        self.position = 0;
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        // Could do something smarter
        self.compact();
        self.data.extend_from_slice(data);
    }

    pub fn extend_from_vec(&mut self, data: Vec<u8>) {
        // TODO: reuse memory
        self.extend_from_slice(&data);
    }

    pub fn extend_from_bytes(&mut self, data: Bytes) {
        // TODO: reuse memory
        self.extend_from_slice(&data);
    }

    pub fn extend_from_bytes_ref(&mut self, data: &Bytes) {
        // TODO: reuse memory
        self.extend_from_slice(&*data);
    }

    pub fn extend_from_iter(&mut self, iter: impl Iterator<Item = u8>) {
        // Could do something smarter
        self.compact();
        self.data.extend(iter);
    }
}

impl Into<Vec<u8>> for WriteBuffer {
    fn into(mut self) -> Vec<u8> {
        self.compact();
        self.data
    }
}

impl Into<Bytes> for WriteBuffer {
    fn into(self) -> Bytes {
        Bytes::from(Into::<Vec<u8>>::into(self))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
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
