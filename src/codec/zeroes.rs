use bytes::Buf;
use std::cmp;
use std::io::IoSlice;

/// Zeroes `Buf`
#[derive(Clone, Copy, Default)]
pub struct Zeroes(pub usize);

/// Padding length is up to 256 bytes, so this buffer is practically enough
static ZEROES: &[u8] = &[0; 256];

impl Buf for Zeroes {
    fn remaining(&self) -> usize {
        self.0
    }

    fn bytes(&self) -> &[u8] {
        let len = cmp::min(self.0, ZEROES.len());
        &ZEROES[..len]
    }

    fn bytes_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        let mut c = self.clone();
        let mut n = 0;
        while c.0 != 0 && n != dst.len() {
            let len = c.bytes().len();
            dst[n] = IoSlice::new(&ZEROES[..len]);
            c.0 -= len;
            n += 1;
        }
        n
    }

    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.0);
        self.0 -= cnt;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn buf_small() {
        let mut z = Zeroes(5);
        assert_eq!(&[0, 0, 0, 0, 0], z.bytes());
        assert_eq!(5, z.remaining());
        z.advance(3);
        assert_eq!(2, z.remaining());
        assert_eq!(&[0, 0], z.bytes());
        z.advance(2);
        assert_eq!(0, z.remaining());
    }

    #[test]
    fn buf_large() {
        let mut z = Zeroes(10000);
        assert_eq!(ZEROES, z.bytes());
        z.advance(ZEROES.len());
        assert_eq!(10000 - ZEROES.len(), z.remaining());
    }

    #[test]
    fn buf_bytes_vectored_small() {
        let z = Zeroes(ZEROES.len() + 1);

        let mut s = [IoSlice::new(&[])];
        assert_eq!(1, z.bytes_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.bytes_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(&[0], &*s[1]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.bytes_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(&[0], &*s[1]);
    }

    #[test]
    fn buf_bytes_vectored_large() {
        let z = Zeroes(10000);

        let mut s = [IoSlice::new(&[])];
        assert_eq!(1, z.bytes_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.bytes_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(ZEROES, &*s[1]);
    }
}
