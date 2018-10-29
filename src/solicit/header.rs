use std::fmt;
use std::iter::FromIterator;
use std::result;
use std::str;
use std::str::FromStr;

use headers_place::HeadersPlace;
use req_resp::RequestOrResponse;

use assert_types::*;

use bytes::Bytes;

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum PseudoHeaderName {
    // 8.1.2.3 Request Pseudo-Header Fields
    Method = 0,
    Scheme = 1,
    Authority = 2,
    Path = 3,

    // 8.1.2.4 Response Pseudo-Header Fields
    Status = 4,
}

impl PseudoHeaderName {
    pub fn name(&self) -> &'static str {
        match *self {
            PseudoHeaderName::Method => ":method",
            PseudoHeaderName::Scheme => ":scheme",
            PseudoHeaderName::Authority => ":authority",
            PseudoHeaderName::Path => ":path",
            PseudoHeaderName::Status => ":status",
        }
    }

    pub fn parse(value: &[u8]) -> HeaderResult<PseudoHeaderName> {
        match value {
            b":method" => Ok(PseudoHeaderName::Method),
            b":scheme" => Ok(PseudoHeaderName::Scheme),
            b":authority" => Ok(PseudoHeaderName::Authority),
            b":path" => Ok(PseudoHeaderName::Path),
            b":status" => Ok(PseudoHeaderName::Status),
            _ => Err(HeaderError::UnknownPseudoHeader),
        }
    }

    pub fn req_or_resp(&self) -> RequestOrResponse {
        match *self {
            PseudoHeaderName::Method => RequestOrResponse::Request,
            PseudoHeaderName::Scheme => RequestOrResponse::Request,
            PseudoHeaderName::Authority => RequestOrResponse::Request,
            PseudoHeaderName::Path => RequestOrResponse::Request,
            PseudoHeaderName::Status => RequestOrResponse::Response,
        }
    }

    pub fn name_bytes(&self) -> Bytes {
        Bytes::from_static(self.name().as_bytes())
    }

    pub fn names(request_or_response: RequestOrResponse) -> &'static [PseudoHeaderName] {
        static REQUEST_HEADERS: &[PseudoHeaderName] = &[
            PseudoHeaderName::Method,
            PseudoHeaderName::Scheme,
            PseudoHeaderName::Authority,
            PseudoHeaderName::Path,
        ];
        static RESPONSE_HEADERS: &[PseudoHeaderName] = &[PseudoHeaderName::Status];
        match request_or_response {
            RequestOrResponse::Request => REQUEST_HEADERS,
            RequestOrResponse::Response => RESPONSE_HEADERS,
        }
    }

    pub fn all_names() -> &'static [PseudoHeaderName] {
        static ALL_HEADERS: &[PseudoHeaderName] = &[
            PseudoHeaderName::Method,
            PseudoHeaderName::Scheme,
            PseudoHeaderName::Authority,
            PseudoHeaderName::Path,
            PseudoHeaderName::Status,
        ];
        ALL_HEADERS
    }
}

#[derive(Default)]
struct PseudoHeaderNameSet {
    headers_mask: u32,
}

impl PseudoHeaderNameSet {
    fn new() -> PseudoHeaderNameSet {
        Default::default()
    }

    fn insert(&mut self, value: PseudoHeaderName) -> bool {
        let contains = self.contains(value);
        self.headers_mask |= 1 << (value as u32);
        !contains
    }

    fn contains(&self, value: PseudoHeaderName) -> bool {
        self.headers_mask & (1 << (value as u32)) != 0
    }
}

#[allow(dead_code)]
pub struct HeaderName(Bytes);

impl<'a> From<&'a str> for HeaderName {
    fn from(_name: &'a str) -> HeaderName {
        unimplemented!()
    }
}

/// A convenience struct representing a part of a header (either the name or the value).
pub struct HeaderPart(Bytes);

impl fmt::Debug for HeaderPart {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)
    }
}

impl From<Vec<u8>> for HeaderPart {
    fn from(vec: Vec<u8>) -> HeaderPart {
        HeaderPart(Bytes::from(vec))
    }
}

impl From<Bytes> for HeaderPart {
    fn from(bytes: Bytes) -> HeaderPart {
        HeaderPart(bytes)
    }
}

impl<'a> From<&'a [u8]> for HeaderPart {
    fn from(buf: &'a [u8]) -> HeaderPart {
        HeaderPart(Bytes::from(buf))
    }
}

impl From<String> for HeaderPart {
    fn from(s: String) -> HeaderPart {
        From::from(s.into_bytes())
    }
}

impl<'a> From<&'a str> for HeaderPart {
    fn from(s: &'a str) -> HeaderPart {
        From::from(s.as_bytes())
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Header {
    pub name: Bytes,
    pub value: Bytes,
}

fn _assert_header_sync_send() {
    assert_sync::<Header>();
    assert_send::<Header>();
}

#[derive(Debug)]
pub enum HeaderError {
    UnknownPseudoHeader,
    EmptyName,
    EmptyValue(PseudoHeaderName),
    IncorrectCharInName,
    UnexpectedPseudoHeader(PseudoHeaderName),
    PseudoHeadersInTrailers,
    PseudoHeadersAfterRegularHeaders,
    MoreThanOnePseudoHeader(PseudoHeaderName),
    MissingPseudoHeader(PseudoHeaderName),
    ConnectionSpecificHeader(&'static str),
    TeCanOnlyContainTrailer,
}

pub type HeaderResult<T> = result::Result<T, HeaderError>;

impl Header {
    /// Creates a new `Header` with the given name and value.
    ///
    /// The name and value need to be convertible into a `HeaderPart`.
    pub fn new<N: Into<HeaderPart>, V: Into<HeaderPart>>(name: N, value: V) -> Header {
        Header {
            name: name.into().0,
            value: value.into().0,
        }
    }

    /// Return a borrowed representation of the `Header` name.
    pub fn name(&self) -> &[u8] {
        &self.name
    }
    /// Return a borrowed representation of the `Header` value.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// name: value
    pub fn format(&self) -> String {
        format!(
            "{}: {}",
            String::from_utf8_lossy(&self.name),
            String::from_utf8_lossy(&self.value)
        )
    }

    pub fn is_preudo_header(&self) -> bool {
        self.name.len() != 0 && self.name[0] == b':'
    }

    pub fn pseudo_header_name(&self) -> HeaderResult<Option<PseudoHeaderName>> {
        if self.is_preudo_header() {
            PseudoHeaderName::parse(self.name()).map(Some)
        } else {
            Ok(None)
        }
    }

    fn validate_header_name_char(b: u8) -> HeaderResult<()> {
        // TODO: restrict more
        if b >= b'A' && b <= b'Z' {
            return Err(HeaderError::IncorrectCharInName);
        }
        Ok(())
    }

    pub fn validate(&self, req_or_resp: RequestOrResponse) -> HeaderResult<()> {
        if self.name.len() == 0 {
            return Err(HeaderError::EmptyName);
        }

        if let Some(h) = self.pseudo_header_name()? {
            if h.req_or_resp() != req_or_resp {
                return Err(HeaderError::UnexpectedPseudoHeader(h));
            }
        }

        for c in &self.name {
            Header::validate_header_name_char(c)?;
        }

        // HTTP/2 does not use the Connection header field to indicate
        // connection-specific header fields; in this protocol, connection-
        // specific metadata is conveyed by other means.  An endpoint MUST NOT
        // generate an HTTP/2 message containing connection-specific header
        // fields; any message containing connection-specific header fields MUST
        // be treated as malformed (Section 8.1.2.6).
        let connection_specific_headers = [
            "connection",
            "keep-alive",
            "proxy-connection",
            "transfer-encoding",
            "upgrade",
        ];
        for s in &connection_specific_headers {
            if self.name == s.as_bytes() {
                return Err(HeaderError::ConnectionSpecificHeader(s));
            }
        }

        if self.name.as_ref() == b"te" {
            // The only exception to this is the TE header field, which MAY be
            // present in an HTTP/2 request; when it is, it MUST NOT contain any
            // value other than "trailers".
            if self.value.as_ref() != b"trailers" {
                return Err(HeaderError::TeCanOnlyContainTrailer);
            }
        }

        Ok(())
    }
}

impl<N: Into<HeaderPart>, V: Into<HeaderPart>> From<(N, V)> for Header {
    fn from(p: (N, V)) -> Header {
        Header::new(p.0, p.1)
    }
}

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Headers {
    // Pseudo-headers stored before regular headers
    headers: Vec<Header>,
    pseudo_count: usize,
}

impl Headers {
    pub fn new() -> Headers {
        Default::default()
    }

    pub fn from_vec(mut headers: Vec<Header>) -> Headers {
        headers.sort_by_key(|h| !h.is_preudo_header());
        let pseudo_count = headers.iter().take_while(|h| h.is_preudo_header()).count();
        Headers {
            headers,
            pseudo_count,
        }
    }

    pub fn from_vec_pseudo_first(headers: Vec<Header>) -> Result<Headers, HeaderError> {
        let mut saw_regular_header = false;
        let mut pseudo_count = 0;
        for header in &headers {
            if header.is_preudo_header() {
                if saw_regular_header {
                    return Err(HeaderError::PseudoHeadersAfterRegularHeaders);
                }
                pseudo_count += 1;
            } else {
                saw_regular_header = true;
            }
        }
        return Ok(Headers {
            headers,
            pseudo_count,
        });
    }

    pub fn iter(&self) -> impl Iterator<Item = &Header> {
        self.headers.iter()
    }

    fn pseudo_headers(&self) -> &[Header] {
        &self.headers[..self.pseudo_count]
    }

    fn regular_headers(&self) -> &[Header] {
        &self.headers[self.pseudo_count..]
    }

    /// Multiline string
    pub fn dump(&self) -> String {
        let mut r = String::new();
        for h in &self.headers {
            r.push_str(&h.format());
            r.push_str("\n");
        }
        r
    }

    pub fn new_get(path: &str) -> Headers {
        Headers::from_vec(vec![
            Header::new(":method", "GET"),
            Header::new(":path", path),
        ])
    }

    pub fn new_post(path: &str) -> Headers {
        Headers::from_vec(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path),
        ])
    }

    pub fn from_status(code: u32) -> Headers {
        Headers::from_vec(vec![Header::new(":status", format!("{}", code))])
    }

    pub fn ok_200() -> Headers {
        Headers::from_status(200)
    }

    pub fn not_found_404() -> Headers {
        Headers::from_status(404)
    }

    pub fn internal_error_500() -> Headers {
        Headers::from_status(500)
    }

    pub fn redirect_302(location: &str) -> Headers {
        let mut headers = Headers::from_status(302);
        headers.add("location", location);
        headers
    }

    pub fn validate(
        &self,
        req_or_resp: RequestOrResponse,
        headers_place: HeadersPlace,
    ) -> HeaderResult<()> {
        let mut pseudo_headers_met = PseudoHeaderNameSet::new();

        for header in self.pseudo_headers() {
            debug_assert!(header.is_preudo_header());

            header.validate(req_or_resp)?;

            let header_name = header.pseudo_header_name()?.unwrap();

            if headers_place == HeadersPlace::Trailing {
                return Err(HeaderError::PseudoHeadersInTrailers);
            }

            if !pseudo_headers_met.insert(header_name) {
                return Err(HeaderError::MoreThanOnePseudoHeader(header_name));
            }

            if header_name == PseudoHeaderName::Path {
                if header.value.is_empty() {
                    return Err(HeaderError::EmptyValue(header_name));
                }
            }
        }

        for header in self.regular_headers() {
            debug_assert!(!header.is_preudo_header());
        }

        if headers_place == HeadersPlace::Initial {
            let required_headers = match req_or_resp {
                // All HTTP/2 requests MUST include exactly one valid value for the
                // ":method", ":scheme", and ":path" pseudo-header fields, unless it is
                // a CONNECT request (Section 8.3).  An HTTP request that omits
                // mandatory pseudo-header fields is malformed (Section 8.1.2.6).
                RequestOrResponse::Request => {
                    &[
                        PseudoHeaderName::Method,
                        PseudoHeaderName::Scheme,
                        PseudoHeaderName::Path,
                    ][..]
                }
                // For HTTP/2 responses, a single ":status" pseudo-header field is
                // defined that carries the HTTP status code field (see [RFC7231],
                // Section 6).  This pseudo-header field MUST be included in all
                // responses; otherwise, the response is malformed (Section 8.1.2.6).
                RequestOrResponse::Response => &[PseudoHeaderName::Status][..],
            };

            for &required in required_headers {
                if !pseudo_headers_met.contains(required) {
                    return Err(HeaderError::MissingPseudoHeader(required));
                }
            }
        }

        Ok(())
    }

    pub fn get_opt<'a>(&'a self, name: &str) -> Option<&'a str> {
        let headers = if name.starts_with(':') {
            self.pseudo_headers()
        } else {
            self.regular_headers()
        };
        headers
            .iter()
            .find(|h| h.name() == name.as_bytes())
            .and_then(|h| str::from_utf8(h.value()).ok())
    }

    pub fn get<'a>(&'a self, name: &str) -> &'a str {
        self.get_opt(name).unwrap()
    }

    pub fn get_opt_parse<I: FromStr>(&self, name: &str) -> Option<I> {
        self.get_opt(name).and_then(|h| h.parse().ok())
    }

    pub fn status(&self) -> u32 {
        self.get_opt_parse(":status").unwrap()
    }

    pub fn path(&self) -> &str {
        self.get(":path")
    }

    pub fn method(&self) -> &str {
        self.get(":method")
    }

    pub fn content_length(&self) -> Option<u64> {
        match self.get_opt("content-length") {
            Some(v) => v.parse().ok(),
            None => None,
        }
    }

    pub fn add(&mut self, name: &str, value: &str) {
        self.add_header(Header::new(name, value));
    }

    pub fn add_header(&mut self, header: Header) {
        if header.is_preudo_header() {
            let pseudo_count = self.pseudo_count;
            self.headers.insert(pseudo_count, header);
            self.pseudo_count += 1;
        } else {
            self.headers.push(header);
        }
    }

    pub fn extend(&mut self, headers: Headers) {
        self.headers.reserve(headers.headers.len());
        for h in headers.headers {
            self.add_header(h);
        }
    }
}

impl FromIterator<Header> for Headers {
    fn from_iter<T: IntoIterator<Item = Header>>(iter: T) -> Headers {
        Headers::from_vec(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod test {
    use solicit::header::Header;

    #[test]
    fn test_partial_eq_of_headers() {
        let fully_static = Header::new(&b":method"[..], &b"GET"[..]);
        let static_name = Header::new(&b":method"[..], b"GET".to_vec());
        let other = Header::new(&b":path"[..], &b"/"[..]);

        assert!(fully_static == static_name);
        assert!(fully_static != other);
        assert!(static_name != other);
    }

    #[test]
    fn test_debug() {
        assert_eq!(
            "Header { name: b\":method\", value: b\"GET\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"GET"[..]))
        );
        assert_eq!(
            "Header { name: b\":method\", value: b\"\\xcd\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"\xcd"[..]))
        );
    }
}
