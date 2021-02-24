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
            if b > i8::MAX as u8 {
                return Err((AsciiError(()), bs));
            }
        }
        Ok(Ascii(bs))
    }

    /// Create an ASCII string from bytes.
    ///
    /// This is a const fn, and we cannot loop in const fn
    /// so no validation here.
    pub const unsafe fn from_bytes_unchecked(bs: Bytes) -> Ascii {
        Ascii(bs)
    }

    pub fn as_str(&self) -> &str {
        unsafe { str::from_utf8_unchecked(self.0.as_ref()) }
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl ops::Deref for Ascii {
    type Target = str;

    fn deref(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.0) }
    }
}

impl AsRef<[u8]> for Ascii {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsRef<str> for Ascii {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Into<Bytes> for Ascii {
    fn into(self) -> Bytes {
        self.0
    }
}

impl fmt::Debug for Ascii {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Ascii {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

#[derive(Debug)]
pub struct AsciiLowerError(());

/// ASCII lowercase string (any ASCII except uppercase letters).
pub struct AsciiLower(Ascii);

impl AsciiLower {
    pub fn new() -> AsciiLower {
        AsciiLower(Ascii::new())
    }

    /// Validate bytes is an ASCII lowercase string.
    pub fn from_bytes(bs: Bytes) -> Result<AsciiLower, (AsciiLowerError, Bytes)> {
        for &b in &bs {
            if !b.is_ascii() || b.is_ascii_uppercase() {
                return Err((AsciiLowerError(()), bs));
            }
        }
        unsafe { Ok(AsciiLower(Ascii::from_bytes_unchecked(bs))) }
    }

    pub const unsafe fn from_bytes_unchecked(bs: Bytes) -> AsciiLower {
        AsciiLower(Ascii::from_bytes_unchecked(bs))
    }
}

impl fmt::Display for AsciiLower {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
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
    use crate::ascii::AsciiLower;
    use bytes::Bytes;

    #[test]
    fn ascii_from_bytes() {
        assert!(Ascii::from_bytes(Bytes::from_static("ю".as_bytes())).is_err());
    }

    #[test]
    fn ascii_lower_from_bytes() {
        assert!(AsciiLower::from_bytes(Bytes::from_static("qю".as_bytes())).is_err());
        assert!(AsciiLower::from_bytes(Bytes::from_static("qQ".as_bytes())).is_err());
        assert!(AsciiLower::from_bytes(Bytes::from_static("q".as_bytes())).is_ok());
        assert!(AsciiLower::from_bytes(Bytes::from_static("-".as_bytes())).is_ok());
        assert!(AsciiLower::from_bytes(Bytes::from_static("".as_bytes())).is_ok());
    }

    #[test]
    fn ascii_lower_display() {
        assert_eq!(
            "foo-bar",
            format!(
                "{}",
                AsciiLower::from_bytes(Bytes::from_static(b"foo-bar")).unwrap()
            ),
        );
    }
}
