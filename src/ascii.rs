#![allow(dead_code)]

use std::ops;
use std::str;

use bytes::Bytes;

/// Bytes wrapper that guarantees that contained bytes are ASCII.
pub struct Ascii(Bytes);

impl Ascii {
    pub fn new() -> Ascii {
        Ascii(Bytes::new())
    }
}

impl ops::Deref for Ascii {
    type Target = str;

    fn deref(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.0) }
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
