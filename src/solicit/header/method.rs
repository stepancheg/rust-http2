use bytes::Bytes;

use crate::HeaderValue;

/// String `"GET"`.
pub const METHOD_GET: HeaderValue =
    unsafe { HeaderValue::from_bytes_unchecked(Bytes::from_static(b"GET")) };
/// String `"POST"`.
pub const METHOD_POST: HeaderValue =
    unsafe { HeaderValue::from_bytes_unchecked(Bytes::from_static(b"POST")) };
