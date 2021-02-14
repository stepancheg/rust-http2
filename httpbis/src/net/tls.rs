use crate::net::socket::SocketStream;
use crate::AnySocketAddr;
use std::io;
use tls_api::TlsStreamWithSocket;

impl<S: SocketStream> SocketStream for TlsStreamWithSocket<S> {
    fn is_tcp(&self) -> bool {
        false
    }

    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()> {
        self.get_socket_ref().set_tcp_nodelay(no_delay)
    }

    fn peer_addr(&self) -> io::Result<AnySocketAddr> {
        self.get_socket_ref().peer_addr()
    }
}
