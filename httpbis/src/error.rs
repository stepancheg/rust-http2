use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::time::Timeout;
use void::Void;

use crate::assert_types::*;
use crate::solicit::error_code::ErrorCode;
use crate::solicit::frame::ParseFrameError;
use crate::solicit::frame::RawHttpFrameType;
use crate::StreamId;

/// An enum representing errors that can arise when performing operations involving an HTTP/2
/// connection.
#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error("I/O error: {0}")]
    IoError(#[source] io::Error),
    #[error("tls-api layer error: {0}")]
    TlsError(#[source] anyhow::Error),
    #[error("HTTP/2 error code: {0}")]
    CodeError(ErrorCode),
    #[error("RST_STREAM received")]
    RstStreamReceived(ErrorCode),
    #[error("Address resolved to empty list")]
    AddrResolvedToEmptyList,
    #[error("Address resolved to more than one address")]
    AddrResolvedToMoreThanOneAddr(Vec<SocketAddr>),
    #[error("The HTTP/2 connection received an invalid HTTP/2 frame")]
    InvalidFrame(String),
    /// Indicates that the local peer has discovered an overflow in the size of one of the
    /// connection flow control window, which is a connection error.
    #[error("Window size overflow")]
    WindowSizeOverflow,
    #[error("Unknown stream id")]
    UnknownStreamId,
    #[error("Cannot connect")]
    UnableToConnect,
    #[error("Malformed response")]
    MalformedResponse,
    #[error("Connection timed out")]
    ConnectionTimeout,
    #[error("Shutdown of local client or server")]
    Shutdown,
    #[error("Request handler panicked")]
    HandlerPanicked(String),
    #[error("Failed to parse frame")]
    ParseFrameError(ParseFrameError),
    #[error("Internal error: {0}")]
    InternalError(String),
    /// Received `PUSH_PROMISE`, but we don't implement it
    /// and explicitly ask not to send it.
    #[error("Unexpected PUSH_PROMISE")]
    UnexpectedPushPromise,
    #[error("User error: {0}")]
    User(String),
    #[error("Client/connection died, reason unknown")]
    DeathReasonUnknown,
    #[error("Client died: {0}")]
    ClientDied(Arc<Error>),
    #[error("Connection died: {0}")]
    ConnDied(Arc<Error>),
    #[error("Client died and reconnect failed")]
    ClientDiedAndReconnectFailed,
    // TODO: use client died
    #[error("Client controller died")]
    ClientControllerDied,
    #[error("Client panicked: {0}")]
    ClientPanicked(String),
    #[error("Client completed without error")]
    ClientCompletedWithoutError,
    #[error("Caller died")]
    CallerDied,
    #[error("EOF from stream")]
    // TODO: meaningless
    EofFromStream,
    #[error("Expecting CONTINUATION frame, got {0}")]
    // TODO: move to separate error type
    ExpectingContinuationGot(RawHttpFrameType),
    #[error("Expecting CONTINUATION frame with different stream id")]
    ExpectingContinuationGotDifferentStreamId(StreamId, StreamId),
    #[error("CONTINUATION frame without headers")]
    ContinuationFrameWithoutHeaders,
    #[error("Initiated stream with server id from client: {0}")]
    InitiatedStreamWithServerIdFromClient(StreamId),
    #[error("Stream id {0} client less than existing stream id {0}")]
    StreamIdLeExistingStream(StreamId, StreamId),
    #[error("Failed to send request to dump state")]
    // TODO: reason
    FailedToSendReqToDumpState,
    #[error("Stream {0} window overflow")]
    StreamInWindowOverflow(StreamId, i32, u32),
    #[error("Connection window overflow")]
    ConnInWindowOverflow(i32, u32),
    #[error("Ping response wrong payload")]
    PingAckOpaqueDataMismatch(u64, u64),
    #[error("GOAWAY after GOAWAY")]
    GoawayAfterGoaway,
    #[error("Got SETTINGS ack without SETTINGS sent")]
    SettingsAckWithoutSettingsSent,
    #[error("GOAWAY")]
    // TODO: explain
    Goaway,
    #[error("GOAWAY received")]
    GoawayReceived,
    #[error("Stream died")]
    // TODO: explain
    PullStreamDied,
    #[error("Payload too large")]
    PayloadTooLarge(u32, u32),
    #[error("Request is made using HTTP/1")]
    RequestIsMadeUsingHttp1,
    #[error("Listen address is not specified")]
    ListenAddrNotSpecified,
    #[error("Cannot send anything to sink, request is closed")]
    CannotSendClosedLocal,
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

impl From<Void> for Error {
    fn from(v: Void) -> Self {
        match v {}
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
