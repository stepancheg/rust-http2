use crate::net::socket::SocketStream;
use crate::AnySocketAddr;
use std::io;
use tls_api::TlsStream;

impl<S: SocketStream> SocketStream for TlsStream<S> {
    fn is_tcp(&self) -> bool {
        self.get_ref().is_tcp()
    }

    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()> {
        self.get_ref().set_tcp_nodelay(no_delay)
    }

    fn peer_addr(&self) -> io::Result<AnySocketAddr> {
        self.get_ref().peer_addr()
    }
}
