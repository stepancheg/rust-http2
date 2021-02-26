use std::any;
use std::fmt;
use std::io;

use crate::AnySocketAddr;
use tls_api::AsyncSocket;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::macros::support::Pin;

/// TCP stream or socket stream; basically any async stream useable in http2
pub trait SocketStream:
    AsyncRead + AsyncWrite + fmt::Debug + any::Any + Send + Unpin + 'static
{
    /// True iff this socket is TCP socket.
    fn is_tcp(&self) -> bool;

    /// Set no delay for TCP socket, return error for non-TCP socket.
    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()>;

    fn peer_addr(&self) -> io::Result<AnySocketAddr>;

    fn into_async_socket(self: Pin<Box<Self>>) -> Pin<Box<dyn AsyncSocket>>;
}

impl<S: SocketStream + ?Sized> SocketStream for Pin<Box<S>> {
    fn is_tcp(&self) -> bool {
        (**self).is_tcp()
    }

    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()> {
        (**self).set_tcp_nodelay(no_delay)
    }

    fn peer_addr(&self) -> io::Result<AnySocketAddr> {
        (**self).peer_addr()
    }

    fn into_async_socket(self: Pin<Box<Self>>) -> Pin<Box<dyn AsyncSocket>> {
        self
    }
}
