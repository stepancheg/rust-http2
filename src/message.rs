use crate::solicit::header::*;

use crate::bytes_ext::BytesDeque;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;

#[derive(Default)]
pub struct SimpleHttpMessage {
    pub headers: Headers,
    pub body: BytesDeque,
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

    pub fn not_found_404(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::not_found_404(),
            body: BytesDeque::copy_from_slice(message.as_bytes()),
        }
    }

    pub fn internal_error_500(message: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::internal_error_500(),
            body: BytesDeque::copy_from_slice(message.as_bytes()),
        }
    }

    pub fn found_200_plain_text(body: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::ok_200(),
            body: BytesDeque::copy_from_slice(body.as_bytes()),
        }
    }

    pub fn redirect_302(location: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::redirect_302(location),
            body: BytesDeque::new(),
        }
    }

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
