use bytes::Bytes;
use crate::solicit::header::Headers;

/// Stream frame content
#[derive(Debug)]
pub enum DataOrHeaders {
    /// HEADERS frame
    Headers(Headers),
    /// DATA frame
    Data(Bytes),
}
