use std::error::Error as std_Error;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::ops::Deref;

use assert_types::*;

use hpack::decoder::DecoderError;

use tls_api;

use tokio_timer::TimeoutError;

use common::sender::SendError;
use solicit::error_code::ErrorCode;
use solicit::frame::ParseFrameError;
use void::Void;
use StreamDead;
use solicit::frame::HttpFrameType;
use StreamId;

/// An enum representing errors that can arise when performing operations involving an HTTP/2
/// connection.
#[derive(Debug)]
pub enum Error {
    /// The underlying IO layer raised an error
    IoError(io::Error),
    TlsError(tls_api::Error),
    CodeError(ErrorCode),
    RstStreamReceived(ErrorCode),
    AddrResolvedToEmptyList,
    AddrResolvedToMoreThanOneAddr,
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
    User(String),
    StdError(Box<std_Error + Sync + Send + 'static>),
    ClientDied(Option<Arc<Error>>),
    ClientDiedAndReconnectFailed,
    ClientControllerDied,
    // TODO: meaningless
    ChannelDied,
    ConnDied,
    ClientPanicked(String),
    ClientCompletedWithoutError,
    SendError(SendError),
    StreamDead(StreamDead),
    CallerDied,
    // TODO: meaningless
    EofFromStream,
    ExpectingContinuationGot(HttpFrameType),
    ExpectingContinuationGotDifferentStreamId(StreamId, StreamId),
    ContinuationFrameWithoutHeaders,
    InitiatedStreamWithServerIdFromClient(StreamId),
    StreamIdLeExistingStream(StreamId, StreamId),
    // TODO: reason
    FailedToSendReqToDumpState,
    // TODO: reason
    OneshotCancelled,
    StreamInWindowOverflow(StreamId, i32, u32),
    ConnInWindowOverflow(i32, u32),
    PingAckOpaqueDataMismatch(u64, u64),
    GoawayAfterGoaway,
    SettingsAckWithoutSettingsSent,
    // TODO: explain
    Goaway,
    GoawayReceived,
    // TODO: explain
    PullStreamDied,
    PayloadTooLarge(u32, u32),
    RequestIsMadeUsingHttp1,
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IoError(_) => write!(f, "Encountered an IO error"),
            Error::TlsError(_) => write!(f, "Encountered TLS error"),
            Error::CodeError(_) => write!(f, "Encountered HTTP named error"),
            Error::RstStreamReceived(_) => write!(f, "Received RST_STREAM from peer"),
            Error::InvalidFrame(..) => write!(f, "Encountered an invalid or unexpected HTTP/2 frame"),
            Error::CompressionError(_) => write!(f, "Encountered an error with HPACK compression"),
            Error::WindowSizeOverflow => write!(f, "The connection flow control window overflowed"),
            Error::UnknownStreamId => write!(f, "Attempted an operation with an unknown HTTP/2 stream ID"),
            Error::UnableToConnect => write!(f, "An error attempting to establish an HTTP/2 connection"),
            Error::MalformedResponse => write!(f, "The received response was malformed"),
            Error::ConnectionTimeout => write!(f, "Connection time out"),
            Error::Shutdown => write!(f, "Local shutdown"),
            Error::HandlerPanicked(_) => write!(f, "Handler panicked"),
            Error::ParseFrameError(_) => write!(f, "Failed to parse frame"),
            Error::NotImplemented(_) => write!(f, "Not implemented"),
            Error::InternalError(_) => write!(f, "Internal error"),
            Error::ClientDied(_) => write!(f, "Client died"),
            Error::ClientPanicked(_) => write!(f, "Client panicked"),
            Error::ClientCompletedWithoutError => write!(f, "Client completed without error"),
            Error::SendError(_) => write!(f, "Failed to write message to stream"),
            Error::CallerDied => write!(f, "Request caller died"),
            Error::StreamDead(_) => write!(f, "Stream dead"),
            Error::StdError(e) => write!(f, "{}", e),
            Error::User(e) => write!(f, "User error: {}", e),
            Error::AddrResolvedToEmptyList => write!(f, "Address resolved to empty list"),
            Error::AddrResolvedToMoreThanOneAddr => write!(f, "Address resolved to more than one address"),
            Error::ClientDiedAndReconnectFailed => write!(f, "Client died and reconnect failed"),
            Error::ClientControllerDied => write!(f, "Client controller died"),
            Error::ChannelDied => write!(f, "Channel died"),
            Error::ConnDied => write!(f, "Conn died"),
            Error::EofFromStream => write!(f, "EOF from stream"),
            Error::ExpectingContinuationGot(t) => write!(f, "Expecting {} got {}", HttpFrameType::Continuation, t),
            Error::ExpectingContinuationGotDifferentStreamId(_, _) => write!(f, "Expecting {} got different stream id", HttpFrameType::Continuation),
            Error::ContinuationFrameWithoutHeaders => write!(f, "{} frame without {}", HttpFrameType::Continuation, HttpFrameType::Headers),
            Error::InitiatedStreamWithServerIdFromClient(stream_id) => write!(f, "Initiated stream with server id from client: {}", stream_id),
            Error::StreamIdLeExistingStream(_, _) => write!(f, "Stream id <= existing stream"),
            Error::FailedToSendReqToDumpState => write!(f, "Failed to send request to dump state"),
            Error::OneshotCancelled => write!(f, "Oneshot cancelled"),
            Error::StreamInWindowOverflow(stream_id, _, _) => write!(f, "Stream {} in windows overflow", stream_id),
            Error::ConnInWindowOverflow(_, _) => write!(f, "Conn in windows overflow"),
            Error::PingAckOpaqueDataMismatch(_, _) => write!(f, "{} ack opaque data mismatch", HttpFrameType::Ping),
            Error::GoawayAfterGoaway => write!(f, "{} after {}", HttpFrameType::Goaway, HttpFrameType::Goaway),
            Error::SettingsAckWithoutSettingsSent => write!(f, "{} ack without {} sent", HttpFrameType::Settings, HttpFrameType::Settings),
            Error::Goaway => write!(f, "{}", HttpFrameType::Goaway),
            Error::GoawayReceived => write!(f, "{} received", HttpFrameType::Goaway),
            Error::PullStreamDied => write!(f, "Pull stream died"),
            Error::PayloadTooLarge(_, _) => write!(f, "Payload too large"),
            Error::RequestIsMadeUsingHttp1 => write!(f, "Request is made using HTTP/1"),
        }
    }
}

impl std_Error for Error {
    fn cause(&self) -> Option<&std_Error> {
        match *self {
            Error::IoError(ref e) => Some(e),
            Error::TlsError(ref e) => Some(e),
            Error::StdError(ref e) => Some(Box::deref(e) as &std_Error),
            _ => None,
        }
    }
}
