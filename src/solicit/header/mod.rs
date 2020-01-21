use std::fmt;
use std::iter::FromIterator;
use std::result;
use std::str;
use std::str::FromStr;

use crate::headers_place::HeadersPlace;
use crate::req_resp::RequestOrResponse;

use crate::assert_types::*;

use bytes::Bytes;

use crate::solicit::header::method::{METHOD_GET, METHOD_POST};
use crate::solicit::header::name::HeaderName;
use crate::solicit::header::name::PseudoHeaderName;
use crate::solicit::header::name::PseudoHeaderNameSet;
use crate::HeaderValue;

pub(crate) mod method;
pub(crate) mod name;
pub(crate) mod value;

/// HTTP/2 header, regular or pseudo-header
#[derive(PartialEq, Eq, Hash, Clone)]
pub struct Header {
    name: HeaderName,
    pub value: HeaderValue,
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name.name())
            .field("value", &self.value)
            .finish()
    }
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
    IncorrectCharInValue,
    HeaderNameNotAscii,
    HeaderValueNotAscii,
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
    /// Create a new `Header` object with exact values of `name` and `value`.
    ///
    /// This function performs header validation, in particular,
    /// header name must be lower case.
    pub fn new_validate(name: Bytes, value: Bytes) -> HeaderResult<Header> {
        let name = HeaderName::new_validate(name).map_err(|(e, _)| e)?;
        Ok(Header {
            name,
            value: HeaderValue::from(value),
        })
    }

    /// Creates a new `Header` with the given name and value.
    ///
    /// The name and value need to be convertible into a `HeaderPart`.
    ///
    /// Header name is converted to lower case.
    /// This function panics if header name is not valid.
    pub fn new<N: Into<HeaderName>, V: Into<HeaderValue>>(name: N, value: V) -> Header {
        Header {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Construct a `:method` header
    fn method(value: impl Into<HeaderValue>) -> Header {
        Header::new(PseudoHeaderName::Method, value.into())
    }

    /// Construct a `:method` `GET` header
    fn method_get() -> Header {
        Header::method(METHOD_GET)
    }

    /// Construct a `:path` header
    fn path(path: impl Into<HeaderValue>) -> Header {
        Header::new(PseudoHeaderName::Path, path.into())
    }

    /// Construct a `:path` header
    fn status(path: impl Into<HeaderValue>) -> Header {
        Header::new(PseudoHeaderName::Status, path.into())
    }

    /// Return a borrowed representation of the `Header` name.
    pub fn name(&self) -> &str {
        self.name.name()
    }
    /// Return a borrowed representation of the `Header` value.
    pub fn value(&self) -> &[u8] {
        self.value.as_slice()
    }

    /// name: value
    pub fn format(&self) -> String {
        format!(
            "{}: {}",
            self.name(),
            String::from_utf8_lossy(self.value.as_slice())
        )
    }

    pub fn is_preudo_header(&self) -> bool {
        self.name.is_pseudo()
    }

    pub fn pseudo_header_name(&self) -> Option<PseudoHeaderName> {
        self.name.pseudo_header_name()
    }

    pub fn validate(&self, req_or_resp: RequestOrResponse) -> HeaderResult<()> {
        if let Some(h) = self.pseudo_header_name() {
            if h.req_or_resp() != req_or_resp {
                return Err(HeaderError::UnexpectedPseudoHeader(h));
            }
        }

        if req_or_resp == RequestOrResponse::Request && self.name() == "te" {
            // The only exception to this is the TE header field, which MAY be
            // present in an HTTP/2 request; when it is, it MUST NOT contain any
            // value other than "trailers".
            if self.value.as_slice() != b"trailers" {
                return Err(HeaderError::TeCanOnlyContainTrailer);
            }
        }

        Ok(())
    }
}

impl<N: Into<HeaderName>, V: Into<HeaderValue>> From<(N, V)> for Header {
    fn from(p: (N, V)) -> Header {
        Header::new(p.0, p.1)
    }
}

/// HTTP message headers (or trailers)
#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Headers {
    // Pseudo-headers stored before regular headers
    headers: Vec<Header>,
    pseudo_count: usize,
}

impl Headers {
    /// Construct empty headers
    pub fn new() -> Headers {
        Default::default()
    }

    /// Construct headers from a vec of individual headers
    pub fn from_vec(mut headers: Vec<Header>) -> Headers {
        headers.sort_by_key(|h| !h.is_preudo_header());
        let pseudo_count = headers.iter().take_while(|h| h.is_preudo_header()).count();
        Headers {
            headers,
            pseudo_count,
        }
    }

    pub(crate) fn from_vec_pseudo_first(headers: Vec<Header>) -> Result<Headers, HeaderError> {
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

    /// Return an iterator over headers.
    ///
    /// Pseudo headers returned first.
    pub fn iter(&self) -> impl Iterator<Item = &Header> {
        self.headers.iter()
    }

    fn pseudo_headers(&self) -> &[Header] {
        &self.headers[..self.pseudo_count]
    }

    fn regular_headers(&self) -> &[Header] {
        &self.headers[self.pseudo_count..]
    }

    /// Dump all headers as multiline string.
    pub fn dump(&self) -> String {
        let mut r = String::new();
        for h in &self.headers {
            r.push_str(&h.format());
            r.push_str("\n");
        }
        r
    }

    /// Construct a `Headers` object with specified `:method` and `:path` headers
    pub fn new_get(path: impl Into<HeaderValue>) -> Headers {
        Headers::from_vec(vec![Header::method_get(), Header::path(path)])
    }

    /// Construct a `Headers` object with specified `:method` and `:path` headers
    pub fn new_post(path: impl Into<HeaderValue>) -> Headers {
        Headers::from_vec(vec![Header::method(METHOD_POST), Header::path(path)])
    }

    /// Construct a `Headers` object with single `:status` header
    pub fn new_status(code: u32) -> Headers {
        Headers::from_vec(vec![Header::status(format!("{}", code))])
    }

    /// Construct `:status 200` headers
    pub fn ok_200() -> Headers {
        Headers::new_status(200)
    }

    /// Construct `:status 404` headers
    pub fn not_found_404() -> Headers {
        Headers::new_status(404)
    }

    /// Construct `:status 500` headers
    pub fn internal_error_500() -> Headers {
        Headers::new_status(500)
    }

    /// Construct `:status 302; location: <location>` headers
    pub fn redirect_302(location: impl Into<HeaderValue>) -> Headers {
        let mut headers = Headers::new_status(302);
        headers.add("location", location);
        headers
    }

    pub(crate) fn validate(
        &self,
        req_or_resp: RequestOrResponse,
        headers_place: HeadersPlace,
    ) -> HeaderResult<()> {
        let mut pseudo_headers_met = PseudoHeaderNameSet::new();

        for header in self.pseudo_headers() {
            debug_assert!(header.is_preudo_header());

            header.validate(req_or_resp)?;

            let header_name = header.pseudo_header_name().unwrap();

            if headers_place == HeadersPlace::Trailing {
                return Err(HeaderError::PseudoHeadersInTrailers);
            }

            if !pseudo_headers_met.insert(header_name) {
                return Err(HeaderError::MoreThanOnePseudoHeader(header_name));
            }

            if header_name == PseudoHeaderName::Path {
                if header.value.as_slice().is_empty() {
                    return Err(HeaderError::EmptyValue(header_name));
                }
            }
        }

        for header in self.regular_headers() {
            header.validate(req_or_resp)?;
            debug_assert!(!header.is_preudo_header());
        }

        if headers_place == HeadersPlace::Initial {
            let required_headers = match req_or_resp {
                // All HTTP/2 requests MUST include exactly one valid value for the
                // ":method", ":scheme", and ":path" pseudo-header fields, unless it is
                // a CONNECT request (Section 8.3).  An HTTP request that omits
                // mandatory pseudo-header fields is malformed (Section 8.1.2.6).
                RequestOrResponse::Request => &[
                    PseudoHeaderName::Method,
                    PseudoHeaderName::Scheme,
                    PseudoHeaderName::Path,
                ][..],
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
            .find(|h| h.name() == name)
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

    // TODO: return bytes, because headers it not require to be valid UTF-8
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

    /// Add a header
    pub fn add(&mut self, name: impl Into<HeaderName>, value: impl Into<HeaderValue>) {
        self.add_header(Header::new(name, value));
    }

    /// Add a header
    pub fn add_header(&mut self, header: Header) {
        if header.is_preudo_header() {
            let pseudo_count = self.pseudo_count;
            self.headers.insert(pseudo_count, header);
            self.pseudo_count += 1;
        } else {
            self.headers.push(header);
        }
    }

    /// Add all headers
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

    use crate::solicit::header::Header;

    #[test]
    fn test_partial_eq_of_headers() {
        let fully_static = Header::new(&b":method"[..], &b"GET"[..]);
        let static_name = Header::new(&b":method"[..], b"GET".to_vec());
        let other = Header::new(&b":path"[..], &b"/"[..]);

        assert_eq!(fully_static, static_name);
        assert_ne!(fully_static, other);
        assert_ne!(static_name, other);
    }

    #[test]
    fn test_debug() {
        assert_eq!(
            "Header { name: \":method\", value: \"GET\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"GET"[..]))
        );
        assert_eq!(
            "Header { name: \":method\", value: \"\\t\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"\t"[..]))
        );
    }
}
