use crate::BufGetBytes;
use bytes::Buf;
use bytes::Bytes;
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

    fn chunk(&self) -> &[u8] {
        let len = cmp::min(self.0, ZEROES.len());
        &ZEROES[..len]
    }

    fn chunks_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        let mut c = self.clone();
        let mut n = 0;
        while c.0 != 0 && n != dst.len() {
            let len = c.chunk().len();
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

impl BufGetBytes for Zeroes {
    fn get_bytes(&mut self, cnt: usize) -> Bytes {
        assert!(cnt <= self.remaining());
        if cnt <= ZEROES.len() {
            self.0 -= cnt;
            Bytes::from_static(&ZEROES[..cnt])
        } else {
            self.take(cnt).get_bytes(cnt)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn buf_small() {
        let mut z = Zeroes(5);
        assert_eq!(&[0, 0, 0, 0, 0], z.chunk());
        assert_eq!(5, z.remaining());
        z.advance(3);
        assert_eq!(2, z.remaining());
        assert_eq!(&[0, 0], z.chunk());
        z.advance(2);
        assert_eq!(0, z.remaining());
    }

    #[test]
    fn buf_large() {
        let mut z = Zeroes(10000);
        assert_eq!(ZEROES, z.chunk());
        z.advance(ZEROES.len());
        assert_eq!(10000 - ZEROES.len(), z.remaining());
    }

    #[test]
    fn buf_bytes_vectored_small() {
        let z = Zeroes(ZEROES.len() + 1);

        let mut s = [IoSlice::new(&[])];
        assert_eq!(1, z.chunks_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.chunks_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(&[0], &*s[1]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.chunks_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(&[0], &*s[1]);
    }

    #[test]
    fn buf_bytes_vectored_large() {
        let z = Zeroes(10000);

        let mut s = [IoSlice::new(&[])];
        assert_eq!(1, z.chunks_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);

        let mut s = [IoSlice::new(&[]), IoSlice::new(&[])];
        assert_eq!(2, z.chunks_vectored(&mut s));
        assert_eq!(ZEROES, &*s[0]);
        assert_eq!(ZEROES, &*s[1]);
    }

    #[test]
    fn buf_get_bytes() {
        assert_eq!(ZEROES.as_ptr(), Zeroes(10).get_bytes(7).as_ptr());
        assert_eq!(&ZEROES[..7], Zeroes(10).get_bytes(7));
        assert_eq!(3, {
            let mut z = Zeroes(10);
            z.get_bytes(7);
            z.remaining()
        });

        assert_eq!(
            ZEROES.as_ptr(),
            Zeroes(ZEROES.len() * 2 + 1).get_bytes(7).as_ptr()
        );
        assert_eq!(&ZEROES[..7], Zeroes(ZEROES.len() * 2 + 1).get_bytes(7));

        assert_eq!(
            vec![0; ZEROES.len() + 2],
            Zeroes(ZEROES.len() * 2 + 1).get_bytes(ZEROES.len() + 2)
        );
    }
}
