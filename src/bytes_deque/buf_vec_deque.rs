use bytes::Buf;
use std::collections::{vec_deque, VecDeque};
use std::io::IoSlice;

#[derive(Debug)]
pub(crate) struct BufVecDeque<B: Buf> {
    deque: VecDeque<B>,
    len: usize,
}

impl<B: Buf> Default for BufVecDeque<B> {
    fn default() -> Self {
        BufVecDeque {
            deque: VecDeque::default(),
            len: 0,
        }
    }
}

impl<B: Buf, I: Into<VecDeque<B>>> From<I> for BufVecDeque<B> {
    fn from(deque: I) -> Self {
        let deque = deque.into();
        let len = deque.iter().map(Buf::remaining).sum();
        BufVecDeque { deque, len }
    }
}

impl<B: Buf> BufVecDeque<B> {
    pub fn new() -> BufVecDeque<B> {
        Default::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn extend(&mut self, bytes: B) {
        if !bytes.has_remaining() {
            return;
        }
        self.len += bytes.remaining();
        self.deque.push_back(bytes);
    }
}

impl<B: Buf> Buf for BufVecDeque<B> {
    fn remaining(&self) -> usize {
        self.len
    }

    fn bytes(&self) -> &[u8] {
        match self.deque.iter().next() {
            Some(b) => {
                assert!(b.has_remaining());
                b.bytes()
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
            n += b.bytes_vectored(&mut dst[n..]);
        }
        n
    }

    fn advance(&mut self, mut cnt: usize) {
        assert!(self.len >= cnt);
        self.len -= cnt;

        while cnt != 0 {
            let front = self.deque.front_mut().unwrap();
            let front_remaining = front.remaining();
            if cnt < front_remaining {
                front.advance(cnt);
                break;
            }

            self.deque.pop_front().unwrap();

            cnt -= front_remaining;
        }
    }
}

impl<B: Buf> IntoIterator for BufVecDeque<B> {
    type Item = B;
    type IntoIter = vec_deque::IntoIter<B>;

    fn into_iter(self) -> Self::IntoIter {
        self.deque.into_iter()
    }
}
