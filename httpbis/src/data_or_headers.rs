use crate::solicit::header::Headers;
use bytes::Bytes;

/// Stream frame content
#[derive(Debug)]
pub enum DataOrHeaders {
    /// HEADERS frame
    Headers(Headers),
    /// DATA frame
    Data(Bytes),
}
