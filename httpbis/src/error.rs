use std::error::Error as std_Error;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::sync::Arc;

use crate::assert_types::*;

use crate::hpack::decoder::DecoderError;

use tls_api;

use crate::common::sender::SendError;
use crate::display_comma_separated::DisplayCommaSeparated;
use crate::solicit::error_code::ErrorCode;
use crate::solicit::frame::HttpFrameType;
use crate::solicit::frame::ParseFrameError;
use crate::solicit::frame::RawHttpFrameType;
use crate::StreamDead;
use crate::StreamId;
use std::net::SocketAddr;
use tokio::time::Timeout;
use void::Void;

/// An enum representing errors that can arise when performing operations involving an HTTP/2
/// connection.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The underlying IO layer raised an error
    IoError(io::Error),
    /// TLS error.
    TlsError(tls_api::Error),
    /// Error code error.
    CodeError(ErrorCode),
    /// `RST_STREAM` received.
    RstStreamReceived(ErrorCode),
    /// Address resolved to empty list.
    AddrResolvedToEmptyList,
    /// Address resolved to more than one address.
    AddrResolvedToMoreThanOneAddr(Vec<SocketAddr>),
    /// The HTTP/2 connection received an invalid HTTP/2 frame
    InvalidFrame(String),
    /// The HPACK decoder was unable to decode a header chunk and raised an error.
    /// Any decoder error is fatal to the HTTP/2 connection as it means that the decoder contexts
    /// will be out of sync.
    CompressionError(DecoderError),
    /// Indicates that the local peer has discovered an overflow in the size of one of the
    /// connection flow control window, which is a connection error.
    WindowSizeOverflow,
    /// Unknown stream id.
    UnknownStreamId,
    /// Cannot connect.
    UnableToConnect,
    /// Malformed response.
    MalformedResponse,
    /// Connection timed out.
    ConnectionTimeout,
    /// Shutdown of local client or server
    Shutdown,
    /// Request handler panicked.
    HandlerPanicked(String),
    /// Failed to parse frame.
    ParseFrameError(ParseFrameError),
    /// Generic internal error.
    // TODO: get rid of it
    InternalError(String),
    /// Received `PUSH_PROMISE`, but we don't implement it
    /// and explicitly ask not to send it.
    UnexpectedPushPromise,
    /// User error
    User(String),
    /// Std error
    StdError(Box<dyn std_Error + Sync + Send + 'static>),
    /// Client/connection died, but reason is not known
    DeathReasonUnknown,
    /// Client died
    ClientDied(Arc<Error>),
    /// Connection died.
    ConnDied(Arc<Error>),
    /// Client died, reconnect failed
    ClientDiedAndReconnectFailed,
    /// Client controller died.
    // TODO: use client died
    ClientControllerDied,
    /// Client panicked.
    ClientPanicked(String),
    /// Client completed without error.
    ClientCompletedWithoutError,
    /// Send failed.
    SendError(SendError),
    /// Stream dead.
    StreamDead(StreamDead),
    /// Called died.
    CallerDied,
    /// End of stream.
    // TODO: meaningless
    EofFromStream,
    /// Expecting `CONTINUATION` frame.
    // TODO: move to separate error type
    ExpectingContinuationGot(RawHttpFrameType),
    /// Expecting `CONTINUATION` frame with different stream id.
    ExpectingContinuationGotDifferentStreamId(StreamId, StreamId),
    /// `CONTINUATION` frame without headers.
    ContinuationFrameWithoutHeaders,
    /// Wrong stream id.
    InitiatedStreamWithServerIdFromClient(StreamId),
    /// Wrong stream id.
    StreamIdLeExistingStream(StreamId, StreamId),
    /// Failed to send request to dump state.
    // TODO: reason
    FailedToSendReqToDumpState,
    /// Stream id windows overflow.
    StreamInWindowOverflow(StreamId, i32, u32),
    /// Connection in windows overflow.
    ConnInWindowOverflow(i32, u32),
    /// Ping response wrong payload.
    PingAckOpaqueDataMismatch(u64, u64),
    /// Goaway after goaway.
    GoawayAfterGoaway,
    /// Got `SETTINGS` ack without `SETTINGS` sent.
    SettingsAckWithoutSettingsSent,
    /// `GOAWAY`
    // TODO: explain
    Goaway,
    /// Received `GOAWAY`
    GoawayReceived,
    /// Stream died.
    // TODO: explain
    PullStreamDied,
    /// Payload too large.
    PayloadTooLarge(u32, u32),
    /// Request is made using HTTP/1
    RequestIsMadeUsingHttp1,
    /// Listen address is not specified.
    ListenAddrNotSpecified,
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

impl<F> From<Timeout<F>> for Error {
    fn from(_err: Timeout<F>) -> Error {
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IoError(e) => write!(f, "IO error: {}", e),
            Error::TlsError(e) => write!(f, "Encountered TLS error: {}", e),
            Error::CodeError(e) => write!(f, "Encountered HTTP named error: {}", e),
            Error::RstStreamReceived(e) => write!(f, "Received RST_STREAM from peer: {}", e),
            Error::InvalidFrame(..) => {
                write!(f, "Encountered an invalid or unexpected HTTP/2 frame")
            }
            Error::CompressionError(_) => {
                // TODO: display
                write!(f, "Encountered an error with HPACK compression")
            }
            Error::WindowSizeOverflow => write!(f, "The connection flow control window overflowed"),
            Error::UnknownStreamId => {
                write!(f, "Attempted an operation with an unknown HTTP/2 stream ID")
            }
            Error::UnableToConnect => {
                write!(f, "An error attempting to establish an HTTP/2 connection")
            }
            Error::MalformedResponse => write!(f, "The received response was malformed"),
            Error::ConnectionTimeout => write!(f, "Connection time out"),
            Error::Shutdown => write!(f, "Local shutdown"),
            Error::HandlerPanicked(e) => write!(f, "Handler panicked: {}", e),
            // TODO: display
            Error::ParseFrameError(_) => write!(f, "Failed to parse frame"),
            Error::UnexpectedPushPromise => write!(
                f,
                "Received unexpected {} frame",
                HttpFrameType::PushPromise
            ),
            Error::InternalError(e) => write!(f, "Internal error: {}", e),
            Error::ClientDied(e) => write!(f, "Client died: {}", e),
            Error::DeathReasonUnknown => write!(f, "Death reason unknown"),
            Error::ClientPanicked(e) => write!(f, "Client panicked: {}", e),
            Error::ClientCompletedWithoutError => write!(f, "Client completed without error"),
            Error::SendError(_) => write!(f, "Failed to write message to stream"),
            Error::CallerDied => write!(f, "Request caller died"),
            Error::StreamDead(_) => write!(f, "Stream dead"),
            Error::StdError(e) => write!(f, "{}", e),
            Error::User(e) => write!(f, "User error: {}", e),
            Error::AddrResolvedToEmptyList => write!(f, "Address resolved to empty list"),
            Error::AddrResolvedToMoreThanOneAddr(a) => write!(
                f,
                "Address resolved to more than one address: {}",
                DisplayCommaSeparated(&a[..])
            ),
            Error::ClientDiedAndReconnectFailed => write!(f, "Client died and reconnect failed"),
            Error::ClientControllerDied => write!(f, "Client controller died"),
            Error::ConnDied(e) => write!(f, "Conn died: {}", e),
            Error::EofFromStream => write!(f, "EOF from stream"),
            Error::ExpectingContinuationGot(t) => {
                write!(f, "Expecting {} got {}", HttpFrameType::Continuation, t)
            }
            Error::ExpectingContinuationGotDifferentStreamId(_, _) => write!(
                f,
                "Expecting {} got different stream id",
                HttpFrameType::Continuation
            ),
            Error::ContinuationFrameWithoutHeaders => write!(
                f,
                "{} frame without {}",
                HttpFrameType::Continuation,
                HttpFrameType::Headers
            ),
            Error::InitiatedStreamWithServerIdFromClient(stream_id) => write!(
                f,
                "Initiated stream with server id from client: {}",
                stream_id
            ),
            Error::StreamIdLeExistingStream(_, _) => write!(f, "Stream id <= existing stream"),
            Error::FailedToSendReqToDumpState => write!(f, "Failed to send request to dump state"),
            Error::StreamInWindowOverflow(stream_id, _, _) => {
                write!(f, "Stream {} in windows overflow", stream_id)
            }
            Error::ConnInWindowOverflow(_, _) => write!(f, "Conn in windows overflow"),
            Error::PingAckOpaqueDataMismatch(_, _) => {
                write!(f, "{} ack opaque data mismatch", HttpFrameType::Ping)
            }
            Error::GoawayAfterGoaway => write!(
                f,
                "{} after {}",
                HttpFrameType::Goaway,
                HttpFrameType::Goaway
            ),
            Error::SettingsAckWithoutSettingsSent => write!(
                f,
                "{} ack without {} sent",
                HttpFrameType::Settings,
                HttpFrameType::Settings
            ),
            Error::Goaway => write!(f, "{}", HttpFrameType::Goaway),
            Error::GoawayReceived => write!(f, "{} received", HttpFrameType::Goaway),
            Error::PullStreamDied => write!(f, "Pull stream died"),
            Error::PayloadTooLarge(_, _) => write!(f, "Payload too large"),
            Error::RequestIsMadeUsingHttp1 => write!(f, "Request is made using HTTP/1"),
            Error::ListenAddrNotSpecified => write!(f, "Listen addr not specified"),
        }
    }
}

impl std::error::Error for Error {
    fn cause(&self) -> Option<&dyn std_Error> {
        match *self {
            Error::IoError(ref e) => Some(e),
            Error::TlsError(ref e) => Some(e),
            Error::StdError(ref e) => Some(Box::deref(e) as &dyn std_Error),
            _ => None,
        }
    }
}

impl Into<io::Error> for Error {
    fn into(self) -> io::Error {
        match self {
            Error::IoError(e) => e,
            e => io::Error::new(io::ErrorKind::Other, e),
        }
    }
}
