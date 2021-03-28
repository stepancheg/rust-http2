use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::solicit::header::*;
use std::mem;

/// Simple HTTP message is headers and body (no trailers).
#[derive(Default)]
pub struct SimpleHttpMessage {
    /// Message headers.
    pub headers: Headers,
    /// Message body.
    pub body: Bytes,
}

impl SimpleHttpMessage {
    /// New empty.
    pub fn new() -> SimpleHttpMessage {
        Default::default()
    }

    /// Multiline string
    pub fn dump(&self) -> String {
        format!(
            "{}\n{}",
            self.headers.dump(),
            String::from_utf8_lossy(&self.body)
        )
    }

    /// Construct HTTP message from a stream of message parts.
    pub fn from_parts<I>(iter: I) -> SimpleHttpMessage
    where
        I: IntoIterator<Item = DataOrHeadersWithFlag>,
    {
        SimpleHttpMessage::from_part_content(iter.into_iter().map(|c| c.content))
    }

    /// Construct HTTP message from a stream of message parts.
    pub fn from_part_content<I>(iter: I) -> SimpleHttpMessage
    where
        I: IntoIterator<Item = DataOrHeaders>,
    {
        let mut r: SimpleHttpMessage = Default::default();
        for c in iter {
            r.add(c);
        }
        r
    }

    /// Create 404 message.
    pub fn not_found_404(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::not_found_404(),
            body: Bytes::copy_from_slice(message.as_bytes()),
        }
    }

    /// Create 500 message.
    pub fn internal_error_500(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::internal_error_500(),
            body: Bytes::copy_from_slice(message.as_bytes()),
        }
    }

    /// Create 200 message.
    pub fn found_200_plain_text(body: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::ok_200(),
            body: Bytes::copy_from_slice(body.as_bytes()),
        }
    }

    /// Create 302 redirect message.
    pub fn redirect_302(location: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::redirect_302(location),
            body: Bytes::new(),
        }
    }

    /// Modify the message.
    pub fn add(&mut self, part: DataOrHeaders) {
        match part {
            DataOrHeaders::Headers(headers) => {
                self.headers.extend(headers);
            }
            DataOrHeaders::Data(data) => {
                let mut body: BytesMut =
                    BytesMut::from(mem::replace(&mut self.body, Bytes::new()).as_ref());
                body.put(data);
                self.body = body.freeze();
            }
        }
    }
}
