use std::fmt;
use std::io;

use crate::AnySocketAddr;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

/// TCP stream or socket stream; basically any async stream useable in http2
pub trait SocketStream: AsyncRead + AsyncWrite + fmt::Debug + Send + Sync + 'static {
    /// True iff this socket is TCP socket.
    fn is_tcp(&self) -> bool;

    /// Set no delay for TCP socket, return error for non-TCP socket.
    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()>;

    fn peer_addr(&self) -> io::Result<AnySocketAddr>;
}
