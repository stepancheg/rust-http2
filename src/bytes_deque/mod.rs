use bytes::Buf;
use bytes::Bytes;
use std::mem;

pub(crate) mod buf_vec_deque;
pub(crate) mod bytes_vec_deque;
use bytes_vec_deque::BytesVecDeque;
use std::io::IoSlice;

#[derive(Debug)]
enum Inner {
    One(Bytes),
    Deque(BytesVecDeque),
}

/// `VecDeque<Bytes>` but slightly more efficient.
#[derive(Debug)]
pub struct BytesDeque(Inner);

impl BytesDeque {
    pub fn new() -> BytesDeque {
        BytesDeque(Inner::One(Bytes::new()))
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            Inner::One(b) => b.len(),
            Inner::Deque(d) => d.len(),
        }
    }

    pub fn extend(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }

        match &mut self.0 {
            Inner::One(one) if one.is_empty() => {
                self.0 = Inner::One(bytes);
            }
            Inner::One(one) => {
                self.0 = Inner::Deque(BytesVecDeque::from(vec![mem::take(one), bytes]));
            }
            Inner::Deque(deque) if deque.len() == 0 => {
                self.0 = Inner::One(bytes);
            }
            Inner::Deque(deque) => {
                deque.extend(bytes);
            }
        }
    }
}

impl Into<Bytes> for BytesDeque {
    fn into(self) -> Bytes {
        match self.0 {
            Inner::One(b) => b,
            Inner::Deque(d) => d.into(),
        }
    }
}

impl Into<Vec<u8>> for BytesDeque {
    fn into(self) -> Vec<u8> {
        match self.0 {
            Inner::One(b) => Vec::from(b.as_ref()),
            Inner::Deque(d) => d.into(),
        }
    }
}

impl Buf for BytesDeque {
    fn remaining(&self) -> usize {
        match &self.0 {
            Inner::One(b) => b.remaining(),
            Inner::Deque(d) => d.remaining(),
        }
    }

    fn bytes(&self) -> &[u8] {
        match &self.0 {
            Inner::One(b) => b.bytes(),
            Inner::Deque(d) => d.bytes(),
        }
    }

    fn bytes_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        match &self.0 {
            Inner::One(b) => b.bytes_vectored(dst),
            Inner::Deque(d) => d.bytes_vectored(dst),
        }
    }

    fn advance(&mut self, cnt: usize) {
        match &mut self.0 {
            Inner::One(b) => b.advance(cnt),
            Inner::Deque(d) => d.advance(cnt),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::thread_rng;
    use rand::Rng;

    fn extend_iter() {
        let mut d = BytesDeque::new();
        let mut reference = Vec::new();

        for _ in 0..10 {
            let bytes = if thread_rng().gen_range(0, 3) == 0 {
                Bytes::new()
            } else {
                let len = thread_rng().gen_range(0, 10);
                let mut v = Vec::new();
                for _ in 0..len {
                    v.push(thread_rng().gen());
                }
                Bytes::from(v)
            };

            reference.extend_from_slice(&bytes);
            d.extend(bytes);
        }

        assert_eq!(reference, Into::<Vec<u8>>::into(d));
    }

    #[test]
    fn extend() {
        for _ in 0..10000 {
            extend_iter();
        }
    }
}
