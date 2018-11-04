/// The enum represents an error code that are used in `RST_STREAM` and `GOAWAY` frames.
/// These are defined in [Section 7](http://http2.github.io/http2-spec/#ErrorCodes) of the HTTP/2
/// spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// The associated condition is not a result of an error. For example, a GOAWAY might include
    /// this code to indicate graceful shutdown of a connection.
    NoError = 0x0,
    /// The endpoint detected an unspecific protocol error. This error is for use when a more
    /// specific error code is not available.
    ProtocolError = 0x1,
    /// The endpoint encountered an unexpected internal error.
    InternalError = 0x2,
    /// The endpoint detected that its peer violated the flow-control protocol.
    FlowControlError = 0x3,
    /// The endpoint sent a SETTINGS frame but did not receive a response in a timely manner. See
    /// Section 6.5.3 ("Settings Synchronization").
    SettingsTimeout = 0x4,
    /// The endpoint received a frame after a stream was half-closed.
    StreamClosed = 0x5,
    /// The endpoint received a frame with an invalid size.
    FrameSizeError = 0x6,
    /// The endpoint refused the stream prior to performing any application processing (see Section
    /// 8.1.4 for details).
    RefusedStream = 0x7,
    /// Used by the endpoint to indicate that the stream is no longer needed.
    Cancel = 0x8,
    /// The endpoint is unable to maintain the header compression context for the connection.
    CompressionError = 0x9,
    /// The connection established in response to a CONNECT request (Section 8.3) was reset or
    /// abnormally closed.
    ConnectError = 0xa,
    /// The endpoint detected that its peer is exhibiting a behavior that might be generating
    /// excessive load.
    EnhanceYourCalm = 0xb,
    /// The underlying transport has properties that do not meet minimum security requirements (see
    /// Section 9.2).
    InadequateSecurity = 0xc,
    /// The endpoint requires that HTTP/1.1 be used instead of HTTP/2.
    Http11Required = 0xd,
}

impl From<u32> for ErrorCode {
    /// Converts the given `u32` number to the appropriate `ErrorCode` variant.
    fn from(code: u32) -> ErrorCode {
        match code {
            0x0 => ErrorCode::NoError,
            0x1 => ErrorCode::ProtocolError,
            0x2 => ErrorCode::InternalError,
            0x3 => ErrorCode::FlowControlError,
            0x4 => ErrorCode::SettingsTimeout,
            0x5 => ErrorCode::StreamClosed,
            0x6 => ErrorCode::FrameSizeError,
            0x7 => ErrorCode::RefusedStream,
            0x8 => ErrorCode::Cancel,
            0x9 => ErrorCode::CompressionError,
            0xa => ErrorCode::ConnectError,
            0xb => ErrorCode::EnhanceYourCalm,
            0xc => ErrorCode::InadequateSecurity,
            0xd => ErrorCode::Http11Required,
            // According to the spec, unknown error codes MAY be treated as equivalent to
            // INTERNAL_ERROR.
            _ => ErrorCode::InternalError,
        }
    }
}

impl AsRef<str> for ErrorCode {
    fn as_ref(&self) -> &str {
        match *self {
            ErrorCode::NoError => "NoError",
            ErrorCode::ProtocolError => "ProtocolError",
            ErrorCode::InternalError => "InternalError",
            ErrorCode::FlowControlError => "FlowControlError",
            ErrorCode::SettingsTimeout => "SettingsTimeout",
            ErrorCode::StreamClosed => "StreamClosed",
            ErrorCode::FrameSizeError => "FrameSizeError",
            ErrorCode::RefusedStream => "RefusedStream",
            ErrorCode::Cancel => "Cancel",
            ErrorCode::CompressionError => "CompressionError",
            ErrorCode::ConnectError => "ConnectError",
            ErrorCode::EnhanceYourCalm => "EnhanceYourCalm",
            ErrorCode::InadequateSecurity => "InadequateSecurity",
            ErrorCode::Http11Required => "Http11Required",
        }
    }
}

impl Into<u32> for ErrorCode {
    #[inline]
    fn into(self) -> u32 {
        self as u32
    }
}
