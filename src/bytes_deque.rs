use bytes::{Bytes, BytesMut};
use std::collections::VecDeque;
use std::mem;

#[derive(Debug)]
struct BytesVecDeque {
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
    fn extend(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }
        self.len += bytes.len();
        self.deque.push_back(bytes);
    }
}

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
            Inner::Deque(d) => d.len,
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
            Inner::Deque(deque) if deque.len == 0 => {
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
