use bytes::{Buf, Bytes, BytesMut};
use std::collections::VecDeque;
use std::io::IoSlice;

#[derive(Debug, Default)]
pub(crate) struct BytesVecDeque {
    deque: VecDeque<Bytes>,
    len: usize,
}

impl<I: Into<VecDeque<Bytes>>> From<I> for BytesVecDeque {
    fn from(deque: I) -> Self {
        let deque = deque.into();
        let len = deque.iter().map(Bytes::len).sum();
        BytesVecDeque { deque, len }
    }
}

impl Into<Bytes> for BytesVecDeque {
    fn into(self) -> Bytes {
        if self.deque.is_empty() || self.len == 0 {
            Bytes::new()
        } else if self.deque.len() == 1 {
            self.deque.into_iter().next().unwrap()
        } else {
            let mut bytes_mut = BytesMut::with_capacity(self.len);
            for bytes in self.deque {
                bytes_mut.extend_from_slice(&bytes);
            }
            bytes_mut.freeze()
        }
    }
}

impl Into<Vec<u8>> for BytesVecDeque {
    fn into(self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.len);
        for b in self.deque {
            v.extend_from_slice(b.as_ref());
        }
        v
    }
}

impl BytesVecDeque {
    fn new() -> BytesVecDeque {
        Default::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn extend(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }
        self.len += bytes.len();
        self.deque.push_back(bytes);
    }
}

impl Buf for BytesVecDeque {
    fn remaining(&self) -> usize {
        self.len
    }

    fn bytes(&self) -> &[u8] {
        match self.deque.iter().next() {
            Some(b) => {
                assert!(!b.is_empty());
                b.as_ref()
            }
            None => &[],
        }
    }

    fn bytes_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        let mut n = 0;
        for b in &self.deque {
            if n == dst.len() {
                break;
            }
            dst[n] = IoSlice::new(b.as_ref());
            n += 1;
        }
        n
    }

    fn advance(&mut self, mut cnt: usize) {
        assert!(self.len >= cnt);
        self.len -= cnt;

        while cnt != 0 {
            let front = self.deque.front_mut().unwrap();
            if cnt < front.len() {
                front.advance(cnt);
                break;
            }

            cnt -= self.deque.pop_front().unwrap().len();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn buf_empty() {
        let d = BytesVecDeque::new();
        assert_eq!(&[0u8; 0], Buf::bytes(&d));
        assert_eq!(0, Buf::remaining(&d));
        assert_eq!(false, Buf::has_remaining(&d));
    }

    #[test]
    fn buf_advance_full() {
        let mut d = BytesVecDeque::new();
        d.extend(Bytes::from_static(b"ab"));
        d.extend(Bytes::from_static(b"cde"));

        assert_eq!(b"ab", Buf::bytes(&d));
        Buf::advance(&mut d, 2);
        assert_eq!(b"cde", Buf::bytes(&d));
        Buf::advance(&mut d, 3);
        assert_eq!(0, Buf::remaining(&d));
        assert_eq!(false, Buf::has_remaining(&d));
    }

    #[test]
    fn buf_advance() {
        let mut d = BytesVecDeque::new();
        d.extend(Bytes::from_static(b"ab"));
        d.extend(Bytes::from_static(b"cde"));

        assert_eq!(b"ab", Buf::bytes(&d));
        Buf::advance(&mut d, 1);
        assert_eq!(b"b", Buf::bytes(&d));
        Buf::advance(&mut d, 3);
        assert_eq!(b"e", Buf::bytes(&d));
        Buf::advance(&mut d, 1);
        assert_eq!(0, Buf::remaining(&d));
        assert_eq!(false, Buf::has_remaining(&d));
    }

    #[test]
    fn buf_bytes_vectored() {
        let mut d = BytesVecDeque::new();
        d.extend(Bytes::from_static(b"ab"));
        d.extend(Bytes::from_static(b"cde"));

        let mut v = [IoSlice::new(&[]); 1];
        assert_eq!(1, d.bytes_vectored(&mut v));
        assert_eq!(b"ab", &*v[0]);

        let mut v = [IoSlice::new(&[]); 2];
        assert_eq!(2, d.bytes_vectored(&mut v));
        assert_eq!(b"ab", &*v[0]);
        assert_eq!(b"cde", &*v[1]);

        let mut v = [IoSlice::new(&[]); 3];
        assert_eq!(2, d.bytes_vectored(&mut v));
        assert_eq!(b"ab", &*v[0]);
        assert_eq!(b"cde", &*v[1]);
    }
}
