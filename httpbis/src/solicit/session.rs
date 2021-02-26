//! Defines the interface for the session-level management of HTTP/2
//! communication. This is effectively an API that allows hooking into an
//! HTTP/2 connection in order to handle events arising on the connection.
//!
//! The module also provides a default implementation for some of the traits.

/// The enum represents all the states that an HTTP/2 stream can be found in.
///
/// Corresponds to [section 5.1.](http://http2.github.io/http2-spec/#rfc.section.5.1) of the spec.
///
/// ```ignore
///    The lifecycle of a stream is shown in Figure 2.
///                            +--------+
///                    send PP |        | recv PP
///                   ,--------|  idle  |--------.
///                  /         |        |         \
///                 v          +--------+          v
///          +----------+          |           +----------+
///          |          |          | send H /  |          |
///   ,------| reserved |          | recv H    | reserved |------.
///   |      | (local)  |          |           | (remote) |      |
///   |      +----------+          v           +----------+      |
///   |          |             +--------+             |          |
///   |          |     recv ES |        | send ES     |          |
///   |   send H |     ,-------|  open  |-------.     | recv H   |
///   |          |    /        |        |        \    |          |
///   |          v   v         +--------+         v   v          |
///   |      +----------+          |           +----------+      |
///   |      |   half   |          |           |   half   |      |
///   |      |  closed  |          | send R /  |  closed  |      |
///   |      | (remote) |          | recv R    | (local)  |      |
///   |      +----------+          |           +----------+      |
///   |           |                |                 |           |
///   |           | send ES /      |       recv ES / |           |
///   |           | send R /       v        send R / |           |
///   |           | recv R     +--------+   recv R   |           |
///   | send R /  `----------->|        |<-----------'  send R / |
///   | recv R                 | closed |               recv R   |
///   `----------------------->|        |<----------------------'
///                            +--------+
///      send:   endpoint sends this frame
///      recv:   endpoint receives this frame
///      H:  HEADERS frame (with implied CONTINUATIONs)
///      PP: PUSH_PROMISE frame (with implied CONTINUATIONs)
///      ES: END_STREAM flag
///      R:  RST_STREAM frame
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamState {
    /// State
    Idle,
    /// State
    ReservedLocal,
    /// State
    ReservedRemote,
    /// State
    Open,
    /// State
    HalfClosedRemote,
    /// State
    HalfClosedLocal,
    /// State
    Closed,
}

impl StreamState {
    /// Returns whether the stream is closed.
    ///
    /// A stream is considered to be closed iff its state is set to `Closed`.
    pub fn is_closed(&self) -> bool {
        *self == StreamState::Closed
    }

    /// Returns whether the stream is closed locally.
    pub fn is_closed_local(&self) -> bool {
        match *self {
            StreamState::HalfClosedLocal | StreamState::Closed => true,
            _ => false,
        }
    }

    /// Returns whether the remote peer has closed the stream. This includes a fully closed stream.
    pub fn is_closed_remote(&self) -> bool {
        match *self {
            StreamState::HalfClosedRemote | StreamState::Closed => true,
            _ => false,
        }
    }
}

/// Stream state which is idle or closed.
pub enum StreamStateIdleOrClosed {
    /// Idle.
    Idle,
    /// Closed.
    Closed,
}

impl From<StreamStateIdleOrClosed> for StreamState {
    fn from(s: StreamStateIdleOrClosed) -> Self {
        match s {
            StreamStateIdleOrClosed::Idle => StreamState::Idle,
            StreamStateIdleOrClosed::Closed => StreamState::Closed,
        }
    }
}
