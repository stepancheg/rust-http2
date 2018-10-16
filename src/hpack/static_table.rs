/// Represents the type of the static table, as defined by the HPACK spec.
pub(crate) struct StaticTable(&'static [(&'static [u8], &'static [u8])]);

impl StaticTable {
    pub fn new() -> StaticTable {
        StaticTable(STATIC_TABLE)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static [u8], &'static [u8])> {
        self.0.iter().map(|h| *h)
    }

    pub fn get_by_index(&self, index: u32) -> Result<(&'static [u8], &'static [u8]), u32> {
        if (index as usize) < self.0.len() {
            Ok(self.0[index as usize])
        } else {
            Err(index - self.0.len() as u32)
        }
    }
}

/// The table represents the static header table defined by the HPACK spec.
/// (HPACK, Appendix A)
pub(crate) static STATIC_TABLE: &'static [(&'static [u8], &'static [u8])] = &[
    (b":authority", b""),
    (b":method", b"GET"),
    (b":method", b"POST"),
    (b":path", b"/"),
    (b":path", b"/index.html"),
    (b":scheme", b"http"),
    (b":scheme", b"https"),
    (b":status", b"200"),
    (b":status", b"204"),
    (b":status", b"206"),
    (b":status", b"304"),
    (b":status", b"400"),
    (b":status", b"404"),
    (b":status", b"500"),
    (b"accept-", b""),
    (b"accept-encoding", b"gzip, deflate"),
    (b"accept-language", b""),
    (b"accept-ranges", b""),
    (b"accept", b""),
    (b"access-control-allow-origin", b""),
    (b"age", b""),
    (b"allow", b""),
    (b"authorization", b""),
    (b"cache-control", b""),
    (b"content-disposition", b""),
    (b"content-encoding", b""),
    (b"content-language", b""),
    (b"content-length", b""),
    (b"content-location", b""),
    (b"content-range", b""),
    (b"content-type", b""),
    (b"cookie", b""),
    (b"date", b""),
    (b"etag", b""),
    (b"expect", b""),
    (b"expires", b""),
    (b"from", b""),
    (b"host", b""),
    (b"if-match", b""),
    (b"if-modified-since", b""),
    (b"if-none-match", b""),
    (b"if-range", b""),
    (b"if-unmodified-since", b""),
    (b"last-modified", b""),
    (b"link", b""),
    (b"location", b""),
    (b"max-forwards", b""),
    (b"proxy-authenticate", b""),
    (b"proxy-authorization", b""),
    (b"range", b""),
    (b"referer", b""),
    (b"refresh", b""),
    (b"retry-after", b""),
    (b"server", b""),
    (b"set-cookie", b""),
    (b"strict-transport-security", b""),
    (b"transfer-encoding", b""),
    (b"user-agent", b""),
    (b"vary", b""),
    (b"via", b""),
    (b"www-authenticate", b""),
];
