use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::sync::Arc;

use assert_types::*;

use hpack::decoder::DecoderError;

use tls_api;

use tokio_timer::TimeoutError;

use common::sender::SendError;
use solicit::error_code::ErrorCode;
use solicit::frame::ParseFrameError;
use void::Void;
use StreamDead;

/// An enum representing errors that can arise when performing operations involving an HTTP/2
/// connection.
#[derive(Debug)]
pub enum Error {
    /// The underlying IO layer raised an error
    IoError(io::Error),
    TlsError(tls_api::Error),
    CodeError(ErrorCode),
    RstStreamReceived(ErrorCode),
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
    ParseFrameError(ParseFrameError),
    InternalError(String),
    NotImplemented(&'static str),
    // TODO: replace with variants
    Other(&'static str),
    ClientDied(Option<Arc<Error>>),
    ClientPanicked(String),
    ClientCompletedWithoutError,
    SendError(SendError),
    StreamDead(StreamDead),
    CallerDied,
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

impl From<ParseFrameError> for Error {
    fn from(e: ParseFrameError) -> Self {
        Error::ParseFrameError(e)
    }
}

impl From<SendError> for Error {
    fn from(e: SendError) -> Self {
        Error::SendError(e)
    }
}

impl From<StreamDead> for Error {
    fn from(e: StreamDead) -> Self {
        Error::StreamDead(e)
    }
}

impl From<Void> for Error {
    fn from(v: Void) -> Self {
        match v {}
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
            Error::RstStreamReceived(_) => "Received RST_STREAM from peer",
            Error::InvalidFrame(..) => "Encountered an invalid or unexpected HTTP/2 frame",
            Error::CompressionError(_) => "Encountered an error with HPACK compression",
            Error::WindowSizeOverflow => "The connection flow control window overflowed",
            Error::UnknownStreamId => "Attempted an operation with an unknown HTTP/2 stream ID",
            Error::UnableToConnect => "An error attempting to establish an HTTP/2 connection",
            Error::MalformedResponse => "The received response was malformed",
            Error::ConnectionTimeout => "Connection time out",
            Error::Shutdown => "Local shutdown",
            Error::HandlerPanicked(_) => "Handler panicked",
            Error::ParseFrameError(_) => "Failed to parse frame",
            Error::NotImplemented(_) => "Not implemented",
            Error::InternalError(_) => "Internal error",
            Error::ClientDied(_) => "Client died",
            Error::ClientPanicked(_) => "Client panicked",
            Error::ClientCompletedWithoutError => "Client completed without error",
            Error::SendError(_) => "Failed to write message to stream",
            Error::CallerDied => "Request caller died",
            Error::StreamDead(_) => "Stream dead",
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
