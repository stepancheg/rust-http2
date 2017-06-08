use bytes::Bytes;

use solicit::header::*;

use stream_part::*;

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
        format!("{}\n{}", self.headers.dump(), String::from_utf8_lossy(&self.body))
    }

    pub fn from_parts<I>(iter: I) -> SimpleHttpMessage
        where I : IntoIterator<Item=HttpStreamPart>
    {
        SimpleHttpMessage::from_part_content(iter.into_iter().map(|c| c.content))
    }

    pub fn from_part_content<I>(iter: I) -> SimpleHttpMessage
        where I : IntoIterator<Item=HttpStreamPartContent>
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
            body: Bytes::from(message),
        }
    }

    pub fn found_200_plain_text(body: &str) -> SimpleHttpMessage {
        SimpleHttpMessage {
            headers: Headers::ok_200(),
            body: Bytes::from(body),
        }
    }

    pub fn add(&mut self, part: HttpStreamPartContent) {
        match part {
            HttpStreamPartContent::Headers(headers) => {
                self.headers.extend(headers);
            }
            HttpStreamPartContent::Data(data) => {
                self.body.extend_from_slice(&data);
            }
        }
    }
}
