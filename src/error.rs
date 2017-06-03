use std::fmt;
use std::io;
use std::error::Error as StdError;

use assert_types::*;

use hpack::decoder::DecoderError;

use tls_api;

use tokio_timer::TimeoutError;

/// The enum represents an error code that are used in `RST_STREAM` and `GOAWAY` frames.
/// These are defined in [Section 7](http://http2.github.io/http2-spec/#ErrorCodes) of the HTTP/2
/// spec.
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// An enum representing errors that can arise when performing operations involving an HTTP/2
/// connection.
#[derive(Debug)]
pub enum Error {
    /// The underlying IO layer raised an error
    IoError(io::Error),
    TlsError(tls_api::Error),
    CodeError(ErrorCode),
    /// The HTTP/2 connection received an invalid HTTP/2 frame
    InvalidFrame(String),
    /// The HPACK decoder was unable to decode a header chunk and raised an error.
    /// Any decoder error is fatal to the HTTP/2 connection as it means that the decoder contexts
    /// will be out of sync.
    CompressionError(DecoderError),
    /// Indicates that the local peer has discovered an overflow in the size of one of the
    /// connection flow control window, which is a connection error.
    WindowSizeOverflow,
    UnknownStreamId,
    UnableToConnect,
    MalformedResponse,
    ConnectionTimeout,
    /// Shutdown of local client or server
    Shutdown,
    HandlerPanicked(String),
    Other(&'static str),
}

fn _assert_error_sync_send() {
    assert_send::<Error>();
    assert_sync::<Error>();
}

/// Implement the trait that allows us to automatically convert `io::Error`s
/// into an `HttpError` by wrapping the given `io::Error` into an `HttpError::IoError` variant.
impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

impl From<tls_api::Error> for Error {
    fn from(error: tls_api::Error) -> Error {
        Error::TlsError(error)
    }
}

impl<F> From<TimeoutError<F>> for Error {
    fn from(_err: TimeoutError<F>) -> Error {
        Error::ConnectionTimeout
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "HTTP/2 Error: {}", self.description())
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::IoError(_) => "Encountered an IO error",
            Error::TlsError(_) => "Encountered TLS error",
            Error::CodeError(_) => "Encountered HTTP named error",
            Error::InvalidFrame(..) => "Encountered an invalid or unexpected HTTP/2 frame",
            Error::CompressionError(_) => "Encountered an error with HPACK compression",
            Error::WindowSizeOverflow => "The connection flow control window overflowed",
            Error::UnknownStreamId => "Attempted an operation with an unknown HTTP/2 stream ID",
            Error::UnableToConnect => "An error attempting to establish an HTTP/2 connection",
            Error::MalformedResponse => "The received response was malformed",
            Error::ConnectionTimeout => "Connection time out",
            Error::Shutdown => "Local shutdown",
            Error::HandlerPanicked(_) => "Handler panicked",
            Error::Other(_) => "An unknown error",
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            Error::IoError(ref e) => Some(e),
            Error::TlsError(ref e) => Some(e),
            _ => None,
        }
    }
}
