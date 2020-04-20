//! Implementation of `get_bytes` operation on `Buf`.
//!
//! See also [this PR](https://github.com/tokio-rs/bytes/pull/363).

use bytes::buf::ext::BufExt;
use bytes::buf::ext::Chain;
use bytes::buf::ext::Take;
use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use std::cmp;

/// Get `Bytes` from `Buf`.
pub trait BufGetBytes: Buf {
    /// Consumes requested number of bytes from `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use bytes::Buf;
    /// use httpbis::BufGetBytes;
    ///
    /// let bytes = (&b"hello world"[..]).get_bytes(5);
    /// assert_eq!(&bytes[..], &b"hello"[..]);
    /// ```
    ///
    /// # Panics
    ///
    /// This function panics if there is not enough remaining data in `self`.
    fn get_bytes(&mut self, mut cnt: usize) -> Bytes {
        if cnt == 0 {
            return Bytes::new();
        }

        assert!(self.remaining() >= cnt);

        // Could be as simple as self.take(count).to_bytes()
        // but compiler stack overflows on instantiation of test Buf impl for some types

        let mut ret = BytesMut::with_capacity(cnt);

        while cnt != 0 {
            let bytes = self.bytes();
            let step = cmp::min(bytes.len(), cnt);
            ret.put_slice(&bytes[..step]);
            self.advance(step);
            cnt -= step;
        }

        ret.freeze()
    }
}

impl BufGetBytes for Bytes {
    fn get_bytes(&mut self, cnt: usize) -> Bytes {
        self.split_to(cnt)
    }
}

impl BufGetBytes for BytesMut {
    fn get_bytes(&mut self, cnt: usize) -> Bytes {
        self.split_to(cnt).freeze()
    }
}

impl BufGetBytes for &[u8] {}

impl<A: BufGetBytes, B: BufGetBytes> BufGetBytes for Chain<A, B> {
    fn get_bytes(&mut self, mut cnt: usize) -> Bytes {
        let a_rem = self.first_mut().remaining();
        if a_rem >= cnt {
            self.first_mut().get_bytes(cnt)
        } else if !self.first_mut().has_remaining() {
            self.last_mut().get_bytes(cnt)
        } else {
            assert!(cnt <= a_rem + self.last_mut().remaining());

            let mut ret = BytesMut::with_capacity(cnt);
            ret.put(self.first_mut());
            ret.put((self.last_mut()).take(cnt - a_rem));
            ret.freeze()
        }
    }
}

impl<A: BufGetBytes> BufGetBytes for Take<A> {
    fn get_bytes(&mut self, mut cnt: usize) -> Bytes {
        assert!(cnt <= self.remaining());
        let ret = self.get_mut().get_bytes(cnt);
        self.set_limit(self.limit() - cnt);
        ret
    }
}

impl<A: BufGetBytes> BufGetBytes for &mut A {
    fn get_bytes(&mut self, cnt: usize) -> Bytes {
        (&mut **self).get_bytes(cnt)
    }
}

#[cfg(test)]
mod test {
    use crate::bytes_ext::buf_get_bytes::BufGetBytes;
    use bytes::buf::BufExt;
    use bytes::Buf;
    use bytes::Bytes;
    use bytes::BytesMut;

    #[test]
    fn get_to_bytes() {
        let mut buf = &b"abcd"[..];
        assert_eq!(Bytes::copy_from_slice(b"abcd"), buf.to_bytes());
        assert_eq!(b"", buf);
    }

    #[test]
    fn get_get_bytes() {
        let mut buf = &b"abcd"[..];
        assert_eq!(Bytes::copy_from_slice(b"abc"), buf.get_bytes(3));
        assert_eq!(b"d", buf);
    }

    #[test]
    fn bytes_get_bytes() {
        let mut bytes = Bytes::from(b"abc".to_vec());

        let a = bytes.get_bytes(1);
        assert_eq!(b"a", a.as_ref());
        // assert returned bytes object is a subslice of original bytes object
        assert_eq!(
            a.as_ref().as_ptr() as usize + 1,
            bytes.as_ref().as_ptr() as usize
        );

        let bc = bytes.get_bytes(2);
        assert_eq!(b"bc", bc.as_ref());
        // assert returned bytes object is a subslice of original bytes object
        assert_eq!(
            bc.as_ref().as_ptr() as usize,
            a.as_ref().as_ptr() as usize + 1
        );

        assert_eq!(Bytes::new(), bytes);
    }

    #[test]
    fn bytes_mut_get_bytes() {
        let mut bytes = BytesMut::from(&b"abc"[..]);

        let a = bytes.get_bytes(1);
        assert_eq!(b"a", a.as_ref());
        // assert returned bytes object is a subslice of original bytes object
        assert_eq!(
            a.as_ref().as_ptr() as usize + 1,
            bytes.as_ref().as_ptr() as usize
        );

        let bc = bytes.get_bytes(2);
        assert_eq!(b"bc", bc.as_ref());
        // assert returned bytes object is a subslice of original bytes object
        assert_eq!(
            bc.as_ref().as_ptr() as usize,
            a.as_ref().as_ptr() as usize + 1
        );

        assert_eq!(BytesMut::new(), bytes);
    }

    #[test]
    fn chain_to_bytes() {
        let mut ab = Bytes::copy_from_slice(b"ab");
        let mut cd = Bytes::copy_from_slice(b"cd");
        let mut chain = (&mut ab).chain(&mut cd);
        assert_eq!(Bytes::copy_from_slice(b"abcd"), chain.to_bytes());
        assert_eq!(Bytes::new(), ab);
        assert_eq!(Bytes::new(), cd);
    }

    #[test]
    fn chain_to_bytes_first_empty() {
        let mut cd = Bytes::copy_from_slice(b"cd");
        let cd_ptr = cd.as_ptr();
        let mut chain = Bytes::new().chain(&mut cd);
        let cd_to_bytes = chain.to_bytes();
        assert_eq!(b"cd", cd_to_bytes.as_ref());
        // assert `to_bytes` did not allocate
        // this is not possible without reimplementing `to_bytes` in `Chain`
        // assert_eq!(cd_ptr, cd_to_bytes.as_ptr());
        // assert_eq!(Bytes::new(), cd);
    }

    #[test]
    fn chain_to_bytes_second_empty() {
        let mut ab = Bytes::copy_from_slice(b"ab");
        let ab_ptr = ab.as_ptr();
        let mut chain = (&mut ab).chain(Bytes::new());
        let ab_to_bytes = chain.to_bytes();
        assert_eq!(b"ab", ab_to_bytes.as_ref());
        // assert `to_bytes` did not allocate
        // this is not possible without reimplementing `to_bytes` in `Chain`
        // assert_eq!(ab_ptr, ab_to_bytes.as_ptr());
        // assert_eq!(Bytes::new(), ab);
    }

    #[test]
    fn chain_get_bytes() {
        let mut ab = Bytes::copy_from_slice(b"ab");
        let mut cd = Bytes::copy_from_slice(b"cd");
        let ab_ptr = ab.as_ptr();
        let cd_ptr = cd.as_ptr();
        let mut chain = (&mut ab).chain(&mut cd);
        let a = chain.get_bytes(1);
        let bc = chain.get_bytes(2);
        let d = chain.get_bytes(1);

        assert_eq!(Bytes::copy_from_slice(b"a"), a);
        assert_eq!(Bytes::copy_from_slice(b"bc"), bc);
        assert_eq!(Bytes::copy_from_slice(b"d"), d);

        // assert `get_bytes` did not allocate
        assert_eq!(ab_ptr, a.as_ptr());
        // assert `get_bytes` did not allocate
        assert_eq!(cd_ptr.wrapping_offset(1), d.as_ptr());
    }

    #[test]
    fn take_to_bytes() {
        let mut abcd = Bytes::copy_from_slice(b"abcd");
        let abcd_ptr = abcd.as_ptr();
        let mut take = (&mut abcd).take(2);
        let ab = take.to_bytes();
        assert_eq!(Bytes::copy_from_slice(b"ab"), ab);
        // assert `to_bytes` did not allocate
        // this is not possible without reimplementing `to_bytes` in `Take`
        //assert_eq!(abcd_ptr, ab.as_ptr());
        //assert_eq!(Bytes::copy_from_slice(b"cd"), abcd);
    }

    #[test]
    fn take_get_bytes() {
        let mut abcd = Bytes::copy_from_slice(b"abcd");
        let abcd_ptr = abcd.as_ptr();
        let mut take = (&mut abcd).take(2);
        let a = take.get_bytes(1);
        assert_eq!(Bytes::copy_from_slice(b"a"), a);
        // assert `to_bytes` did not allocate
        assert_eq!(abcd_ptr, a.as_ptr());
        assert_eq!(Bytes::copy_from_slice(b"bcd"), abcd);
    }

    #[test]
    #[should_panic]
    fn take_get_bytes_panics() {
        let abcd = Bytes::copy_from_slice(b"abcd");
        abcd.take(2).get_bytes(3);
    }
}
