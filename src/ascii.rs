#![allow(dead_code)]

use std::ops;
use std::str;

use bytes::Bytes;
use std::fmt;
use std::str::Utf8Error;

/// Bytes wrapper that guarantees that contained bytes are ASCII.
#[derive(Eq, PartialEq, Hash, Clone)]
pub struct Ascii(Bytes);

impl Ascii {
    pub fn new() -> Ascii {
        Ascii(Bytes::new())
    }

    pub fn from_utf8(b: Bytes) -> Result<Ascii, (Utf8Error, Bytes)> {
        if let Err(e) = str::from_utf8(&b) {
            return Err((e, b));
        }
        Ok(Ascii(b))
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
