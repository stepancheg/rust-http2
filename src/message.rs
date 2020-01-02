use bytes::{Bytes, BytesMut};

use crate::solicit::header::*;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;

#[derive(Default)]
pub struct SimpleHttpMessage {
    pub headers: Headers,
    pub body: Bytes,
}

impl SimpleHttpMessage {
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

    pub fn from_parts<I>(iter: I) -> SimpleHttpMessage
    where
        I: IntoIterator<Item = DataOrHeadersWithFlag>,
    {
        SimpleHttpMessage::from_part_content(iter.into_iter().map(|c| c.content))
    }

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

    pub fn not_found_404(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::not_found_404(),
            body: Bytes::copy_from_slice(message.as_bytes()),
        }
    }

    pub fn internal_error_500(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::internal_error_500(),
            body: Bytes::copy_from_slice(message.as_bytes()),
        }
    }

    pub fn found_200_plain_text(body: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::ok_200(),
            body: Bytes::copy_from_slice(body.as_bytes()),
        }
    }

    pub fn redirect_302(location: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::redirect_302(location),
            body: Bytes::new(),
        }
    }

    pub fn add(&mut self, part: DataOrHeaders) {
        match part {
            DataOrHeaders::Headers(headers) => {
                self.headers.extend(headers);
            }
            DataOrHeaders::Data(data) => {
                // TODO: quadratic
                let mut body = BytesMut::new();
                body.extend_from_slice(&self.body[..]);
                body.extend_from_slice(&data[..]);
                self.body = body.freeze();
            }
        }
    }
}
