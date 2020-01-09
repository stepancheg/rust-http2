#![allow(dead_code)]

use std::ops;
use std::str;

use bytes::Bytes;
use std::fmt;

#[derive(Debug)]
pub struct AsciiError(());

/// Bytes wrapper that guarantees that contained bytes are ASCII.
#[derive(Eq, PartialEq, Hash, Clone)]
pub struct Ascii(Bytes);

impl Ascii {
    pub fn new() -> Ascii {
        Ascii(Bytes::new())
    }

    pub fn from_bytes(bs: Bytes) -> Result<Ascii, (AsciiError, Bytes)> {
        for &b in &bs {
            if b > i8::max_value() as u8 {
                return Err((AsciiError(()), bs));
            }
        }
        Ok(Ascii(bs))
    }

    pub fn as_str(&self) -> &str {
        unsafe { str::from_utf8_unchecked(self.0.as_ref()) }
    }
}

impl ops::Deref for Ascii {
    type Target = str;

    fn deref(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.0) }
    }
}

impl fmt::Debug for Ascii {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

pub struct AsciiLower(Ascii);

impl AsciiLower {
    pub fn new() -> AsciiLower {
        AsciiLower(Ascii::new())
    }
}

impl ops::Deref for AsciiLower {
    type Target = str;

    fn deref(&self) -> &str {
        self.0.deref()
    }
}

impl<'a> From<&'a str> for AsciiLower {
    fn from(_s: &'a str) -> AsciiLower {
        unimplemented!()
    }
}

#[cfg(test)]
mod test {
    use crate::ascii::Ascii;
    use bytes::Bytes;

    #[test]
    fn from_utf8() {
        assert!(Ascii::from_bytes(Bytes::from_static("ю".as_bytes())).is_err());
    }
}
