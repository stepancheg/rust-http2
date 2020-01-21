use crate::ascii::Ascii;
use crate::req_resp::RequestOrResponse;
use crate::solicit::header::{HeaderError, HeaderResult};
use bytes::Bytes;
use bytes::BytesMut;
use std::fmt;

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

impl fmt::Display for PseudoHeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.name(), f)
    }
}

#[derive(Default)]
pub(crate) struct PseudoHeaderNameSet {
    headers_mask: u32,
}

impl PseudoHeaderNameSet {
    pub fn new() -> PseudoHeaderNameSet {
        Default::default()
    }

    pub fn insert(&mut self, value: PseudoHeaderName) -> bool {
        let contains = self.contains(value);
        self.headers_mask |= 1 << (value as u32);
        !contains
    }

    pub fn contains(&self, value: PseudoHeaderName) -> bool {
        self.headers_mask & (1 << (value as u32)) != 0
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
struct RegularHeaderName(Ascii);

impl RegularHeaderName {
    pub fn from_bytes(bs: Bytes) -> Result<RegularHeaderName, (HeaderError, Bytes)> {
        // https://svn.tools.ietf.org/svn/wg/httpbis/specs/rfc7230.html#header.fields
        //
        // field-name     = token
        //
        // token          = 1*<any CHAR except CTLs or separators>
        // separators     = "(" | ")" | "<" | ">" | "@"
        //                | "," | ";" | ":" | "\" | <">
        //                | "/" | "[" | "]" | "?" | "="
        //                | "{" | "}" | SP | HT
        //
        // or?
        //
        // token          = 1*tchar
        // tchar          = "!" / "#" / "$" / "%" / "&" / "'" / "*"
        //                / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
        //                / DIGIT / ALPHA
        //                ; any VCHAR, except delimiters

        if bs.is_empty() {
            return Err((HeaderError::EmptyName, bs));
        }

        for &b in &bs {
            if !b.is_ascii() {
                return Err((HeaderError::HeaderNameNotAscii, bs));
            }
            if b.is_ascii_control() {
                return Err((HeaderError::IncorrectCharInName, bs));
            }
            if b.is_ascii_uppercase() {
                return Err((HeaderError::IncorrectCharInName, bs));
            }
            let bad_chars = b"()<>@,;:\\\"/[]?={} \t";
            if bad_chars.contains(&b) {
                return Err((HeaderError::IncorrectCharInName, bs));
            }
        }
        unsafe { Ok(RegularHeaderName(Ascii::from_bytes_unchecked(bs))) }
    }

    pub const unsafe fn _from_bytes_unchecked(bs: Bytes) -> RegularHeaderName {
        RegularHeaderName(Ascii::from_bytes_unchecked(bs))
    }
}

impl fmt::Display for RegularHeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Separate type to hide contents from rustdoc.
#[derive(Eq, PartialEq, Hash, Clone)]
enum HeaderNameEnum {
    Pseudo(PseudoHeaderName),
    Regular(RegularHeaderName),
}

impl fmt::Display for HeaderNameEnum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeaderNameEnum::Pseudo(p) => fmt::Display::fmt(p, f),
            HeaderNameEnum::Regular(r) => fmt::Display::fmt(r, f),
        }
    }
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
        HeaderName::new(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl<'a> From<&'a [u8]> for HeaderName {
    fn from(s: &'a [u8]) -> Self {
        HeaderName::new(Bytes::copy_from_slice(s))
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

            HeaderName(HeaderNameEnum::Regular(RegularHeaderName::from_bytes(
                name,
            )?))
        })
    }

    /// Return a header name as a string.
    pub fn name(&self) -> &str {
        match &self.0 {
            HeaderNameEnum::Pseudo(p) => p.name(),
            HeaderNameEnum::Regular(r) => r.0.as_str(),
        }
    }

    /// If header name is pseudo header name
    pub fn is_pseudo(&self) -> bool {
        match self.0 {
            HeaderNameEnum::Pseudo(_) => true,
            HeaderNameEnum::Regular(_) => false,
        }
    }

    /// Obtain pseudo header name from this header name
    pub fn pseudo_header_name(&self) -> Option<PseudoHeaderName> {
        match self.0 {
            HeaderNameEnum::Pseudo(p) => Some(p),
            HeaderNameEnum::Regular(_) => None,
        }
    }
}

impl fmt::Debug for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HeaderName({:?})", self.name())
    }
}

impl fmt::Display for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

fn make_ascii_lowercase(bytes: &mut Bytes) {
    if bytes.as_ref().iter().all(|c| !c.is_ascii_uppercase()) {
        return;
    }
    let mut bytes_mut = BytesMut::from(&bytes[..]);
    bytes_mut.as_mut().make_ascii_lowercase();
    *bytes = bytes_mut.freeze();
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn header_name_new_to_lower() {
        assert_eq!("content-type", HeaderName::new("Content-Type").name());
    }

    #[test]
    fn header_name_display() {
        assert_eq!(
            ":method",
            format!(
                "{}",
                HeaderName(HeaderNameEnum::Pseudo(PseudoHeaderName::Method))
            )
        );
        assert_eq!(
            "x-fgfg",
            format!(
                "{}",
                HeaderName(HeaderNameEnum::Regular(
                    RegularHeaderName::from_bytes(Bytes::from("x-fgfg")).unwrap()
                ))
            )
        );
    }

    #[test]
    fn header_name_debug() {
        assert_eq!(
            "HeaderName(\":method\")",
            format!(
                "{:?}",
                HeaderName(HeaderNameEnum::Pseudo(PseudoHeaderName::Method))
            )
        );
        assert_eq!(
            "HeaderName(\"x-fgfg\")",
            format!(
                "{:?}",
                HeaderName(HeaderNameEnum::Regular(
                    RegularHeaderName::from_bytes(Bytes::from("x-fgfg")).unwrap()
                ))
            )
        );
    }
}
