use crate::solicit::header::*;

use crate::bytes_ext::bytes_deque::BytesDeque;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;

/// Simple HTTP message is headers and body.
#[derive(Default)]
pub struct SimpleHttpMessage {
    pub headers: Headers,
    pub body: BytesDeque,
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
            String::from_utf8_lossy(&self.body.get_bytes())
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

    /// Create 404 message.
    pub fn not_found_404(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::not_found_404(),
            body: BytesDeque::copy_from_slice(message.as_bytes()),
        }
    }

    /// Create 500 message.
    pub fn internal_error_500(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::internal_error_500(),
            body: BytesDeque::copy_from_slice(message.as_bytes()),
        }
    }

    /// Create 200 message.
    pub fn found_200_plain_text(body: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::ok_200(),
            body: BytesDeque::copy_from_slice(body.as_bytes()),
        }
    }

    /// Create 302 redirect message.
    pub fn redirect_302(location: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::redirect_302(location),
            body: BytesDeque::new(),
        }
    }

    /// Modify the message.
    pub fn add(&mut self, part: DataOrHeaders) {
        match part {
            DataOrHeaders::Headers(headers) => {
                self.headers.extend(headers);
            }
            DataOrHeaders::Data(data) => {
                self.body.extend(data);
            }
        }
    }
}
