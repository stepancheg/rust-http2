// A HEADERS frame (and associated CONTINUATION frames) can only appear at the start
// or end of a stream. An endpoint that receives a HEADERS frame without
// the END_STREAM flag set after receiving a final (non-informational) status code
// MUST treat the corresponding request or response as malformed (Section 8.1.2.6).
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum HeadersPlace {
    // Initial headers including continuation
    Initial,
    Trailing,
}
