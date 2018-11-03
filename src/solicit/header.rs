use std::fmt;
use std::iter::FromIterator;
use std::result;
use std::str;
use std::str::FromStr;

use headers_place::HeadersPlace;
use req_resp::RequestOrResponse;

use assert_types::*;

use ascii::Ascii;
use bytes::Bytes;
use bytes::BytesMut;
use std::mem;

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

/// A convenience struct representing a header value.
pub struct HeaderValue(Bytes);

impl fmt::Debug for HeaderValue {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)
    }
}

impl From<Vec<u8>> for HeaderValue {
    fn from(vec: Vec<u8>) -> HeaderValue {
        HeaderValue(Bytes::from(vec))
    }
}

impl From<Bytes> for HeaderValue {
    fn from(bytes: Bytes) -> HeaderValue {
        HeaderValue(bytes)
    }
}

impl<'a> From<&'a [u8]> for HeaderValue {
    fn from(buf: &'a [u8]) -> HeaderValue {
        HeaderValue(Bytes::from(buf))
    }
}

impl From<String> for HeaderValue {
    fn from(s: String) -> HeaderValue {
        From::from(s.into_bytes())
    }
}

impl<'a> From<&'a str> for HeaderValue {
    fn from(s: &'a str) -> HeaderValue {
        From::from(s.as_bytes())
    }
}

/// Separate type to hide contents from rustdoc.
#[derive(Eq, PartialEq, Hash, Clone)]
enum HeaderNameEnum {
    Pseudo(PseudoHeaderName),
    Regular(Ascii),
}

/// Representation of header name
///
/// Contained value is guaranteed to contain a valid header name.
///
/// # Examples
///
/// ```
/// # use httpbis::*;
/// assert_eq!("content-type", HeaderName::new("Content-Type").name());
/// assert_eq!(":method", HeaderName::pseudo(PseudoHeaderName::Method).name());
/// ```
#[derive(Eq, PartialEq, Hash, Clone)]
pub struct HeaderName(HeaderNameEnum);

impl From<PseudoHeaderName> for HeaderName {
    fn from(p: PseudoHeaderName) -> Self {
        HeaderName(HeaderNameEnum::Pseudo(p))
    }
}

impl<'a> From<&'a str> for HeaderName {
    fn from(s: &'a str) -> Self {
        HeaderName::new(s)
    }
}

impl<'a> From<&'a [u8]> for HeaderName {
    fn from(s: &'a [u8]) -> Self {
        HeaderName::new(s)
    }
}

impl From<String> for HeaderName {
    fn from(s: String) -> Self {
        HeaderName::new(s)
    }
}

impl From<Vec<u8>> for HeaderName {
    fn from(s: Vec<u8>) -> Self {
        HeaderName::new(s)
    }
}

impl From<Bytes> for HeaderName {
    fn from(s: Bytes) -> Self {
        HeaderName::new(s)
    }
}

impl HeaderName {
    /// Construct a pseudo header name.
    ///
    /// # Example
    ///
    /// ```
    /// # use httpbis::*;
    /// assert_eq!(":method", HeaderName::pseudo(PseudoHeaderName::Method).name());
    /// ```
    pub fn pseudo(name: PseudoHeaderName) -> HeaderName {
        HeaderName(HeaderNameEnum::Pseudo(name))
    }

    /// Construct a header name from string
    ///
    /// # Example
    ///
    /// Default construction
    ///
    /// ```
    /// # use httpbis::*;
    /// assert_eq!("content-type", HeaderName::new("Content-Type").name());
    /// assert_eq!(":method", HeaderName::new(":method").name());
    /// ```
    ///
    /// # Panics
    ///
    /// Panics on incorrect header name.
    ///
    /// ```should_panic
    /// # use httpbis::*;
    /// HeaderName::new("");
    /// ```
    pub fn new(name: impl Into<Bytes>) -> HeaderName {
        let mut name = name.into();
        make_ascii_lowercase(&mut name);
        match HeaderName::new_validate(name) {
            Ok(h) => h,
            Err((e, name)) => panic!("incorrect header name: {:?}: {:?}", name, e),
        }
    }

    /// Construct a header from given sequence of bytes
    ///
    /// Returns error if header name is not valid,
    /// in particular if name is upper case.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate httpbis;
    /// # extern crate bytes;
    /// # use httpbis::*;
    /// # use bytes::*;
    /// assert!(HeaderName::new_validate(Bytes::from(":method")).is_ok());
    /// assert!(HeaderName::new_validate(Bytes::from("Content-Type")).is_err());
    /// ```
    pub fn new_validate(name: Bytes) -> Result<HeaderName, (HeaderError, Bytes)> {
        if name.len() == 0 {
            return Err((HeaderError::EmptyName, name));
        }

        Ok(if name[0] == b':' {
            HeaderName(HeaderNameEnum::Pseudo(
                PseudoHeaderName::parse(&name).map_err(|e| (e, name))?,
            ))
        } else {
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
                if name == s.as_bytes() {
                    return Err((HeaderError::ConnectionSpecificHeader(s), name));
                }
            }

            for b in &name {
                // TODO: restrict more
                if b >= b'A' && b <= b'Z' {
                    return Err((HeaderError::IncorrectCharInName, name.clone()));
                }
            }

            let ascii =
                Ascii::from_utf8(name).map_err(|(_, b)| (HeaderError::HeaderNameNotAscii, b))?;
            HeaderName(HeaderNameEnum::Regular(ascii))
        })
    }

    /// Return a header name as a string.
    pub fn name(&self) -> &str {
        match &self.0 {
            HeaderNameEnum::Pseudo(p) => p.name(),
            HeaderNameEnum::Regular(r) => r.as_str(),
        }
    }

    fn is_pseudo(&self) -> bool {
        match self.0 {
            HeaderNameEnum::Pseudo(_) => true,
            HeaderNameEnum::Regular(_) => false,
        }
    }
}

impl fmt::Debug for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            HeaderNameEnum::Pseudo(p) => fmt::Debug::fmt(p.name(), f),
            HeaderNameEnum::Regular(r) => fmt::Debug::fmt(r, f),
        }
    }
}

/// HTTP/2 header, regular or pseudo-header
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Header {
    name: HeaderName,
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
    HeaderNameNotAscii,
    UnexpectedPseudoHeader(PseudoHeaderName),
    PseudoHeadersInTrailers,
    PseudoHeadersAfterRegularHeaders,
    MoreThanOnePseudoHeader(PseudoHeaderName),
    MissingPseudoHeader(PseudoHeaderName),
    ConnectionSpecificHeader(&'static str),
    TeCanOnlyContainTrailer,
}

pub type HeaderResult<T> = result::Result<T, HeaderError>;

fn make_ascii_lowercase(bytes: &mut Bytes) {
    if bytes.as_ref().iter().all(|c| c.is_ascii_lowercase()) {
        return;
    }
    let mut bytes_mut = BytesMut::from(mem::replace(bytes, Bytes::new()));
    bytes_mut.as_mut().make_ascii_lowercase();
    mem::replace(bytes, bytes_mut.freeze());
}

impl Header {
    /// Create a new `Header` object with exact values of `name` and `value`.
    ///
    /// This function performs header validation, in particular,
    /// header name must be lower case.
    pub fn new_validate(name: Bytes, value: Bytes) -> HeaderResult<Header> {
        let name = HeaderName::new_validate(name).map_err(|(e, _)| e)?;
        Ok(Header { name, value })
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
            value: value.into().0,
        }
    }

    /// Construct a `:method` header
    fn method(value: impl Into<HeaderValue>) -> Header {
        Header::new(PseudoHeaderName::Method, value.into())
    }

    /// Construct a `:method` `GET` header
    fn method_get() -> Header {
        Header::method("GET".as_bytes())
    }

    /// Construct a `:path` header
    fn path(path: impl Into<HeaderValue>) -> Header {
        Header::new(PseudoHeaderName::Path, path.into())
    }

    /// Return a borrowed representation of the `Header` name.
    pub fn name(&self) -> &str {
        self.name.name()
    }
    /// Return a borrowed representation of the `Header` value.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// name: value
    pub fn format(&self) -> String {
        format!("{}: {}", self.name(), String::from_utf8_lossy(&self.value))
    }

    pub fn is_preudo_header(&self) -> bool {
        self.name.is_pseudo()
    }

    pub fn pseudo_header_name(&self) -> Option<PseudoHeaderName> {
        match &self.name.0 {
            HeaderNameEnum::Pseudo(p) => Some(*p),
            HeaderNameEnum::Regular(_) => None,
        }
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
            if self.value.as_ref() != b"trailers" {
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
        Headers::from_vec(vec![
            Header::new(":method", "POST"),
            Header::new(":path", path),
        ])
    }

    /// Construct a `Headers` object with single `:status` header
    pub fn new_status(code: u32) -> Headers {
        Headers::from_vec(vec![Header::new(":status", format!("{}", code))])
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
                if header.value.is_empty() {
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
    use ascii::Ascii;
    use bytes::Bytes;
    use solicit::header::Header;
    use solicit::header::HeaderName;
    use solicit::header::HeaderNameEnum;
    use solicit::header::PseudoHeaderName;

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
    fn test_header_name_debug() {
        assert_eq!(
            "\":method\"",
            format!(
                "{:?}",
                HeaderName(HeaderNameEnum::Pseudo(PseudoHeaderName::Method))
            )
        );
        assert_eq!(
            "\"x-fgfg\"",
            format!(
                "{:?}",
                HeaderName(HeaderNameEnum::Regular(
                    Ascii::from_utf8(Bytes::from("x-fgfg")).unwrap()
                ))
            )
        );
    }

    #[test]
    fn test_debug() {
        assert_eq!(
            "Header { name: \":method\", value: b\"GET\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"GET"[..]))
        );
        assert_eq!(
            "Header { name: \":method\", value: b\"\\xcd\" }",
            format!("{:?}", Header::new(&b":method"[..], &b"\xcd"[..]))
        );
    }
}
