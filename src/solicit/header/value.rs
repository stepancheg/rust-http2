use bytes::Bytes;
use std::fmt;

/// A convenience struct representing a header value.
#[derive(Eq, PartialEq, Hash, Clone)]
pub struct HeaderValue(Bytes);

impl HeaderValue {
    pub fn into_inner(self) -> Bytes {
        self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for HeaderValue {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)
    }
}

impl From<Vec<u8>> for HeaderValue {
    fn from(vec: Vec<u8>) -> HeaderValue {
        HeaderValue(Bytes::from(vec))
    }
}

impl From<Bytes> for HeaderValue {
    fn from(bytes: Bytes) -> HeaderValue {
        HeaderValue(bytes)
    }
}

impl<'a> From<&'a [u8]> for HeaderValue {
    fn from(buf: &'a [u8]) -> HeaderValue {
        HeaderValue(Bytes::copy_from_slice(buf))
    }
}

impl From<String> for HeaderValue {
    fn from(s: String) -> HeaderValue {
        From::from(s.into_bytes())
    }
}

impl<'a> From<&'a str> for HeaderValue {
    fn from(s: &'a str) -> HeaderValue {
        From::from(s.as_bytes())
    }
}
