use std::fmt;
use std::io;

use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

/// TCP stream or socket stream; basically any async stream useable in http2
pub trait SocketStream: AsyncRead + AsyncWrite + fmt::Debug + Send + Sync {
    fn is_tcp(&self) -> bool;

    fn set_nodelay(&self, no_delay: bool) -> io::Result<()>;
}
