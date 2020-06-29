use crate::net::unix::SocketAddrUnix;
use std::net::SocketAddr;
use std::{fmt, io};

// Any socket address: TCP or Unix
#[derive(Clone, Debug, PartialEq)]
pub enum AnySocketAddr {
    Inet(SocketAddr),
    Unix(SocketAddrUnix),
}

impl fmt::Display for AnySocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AnySocketAddr::Inet(ref inet_addr) => fmt::Display::fmt(inet_addr, f),
            AnySocketAddr::Unix(ref unix_addr) => fmt::Display::fmt(unix_addr, f),
        }
    }
}

impl AnySocketAddr {
    /// Get port number of TCP socket, or error from Unix socket.
    pub fn port(&self) -> io::Result<u16> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => Ok(inet_addr.port()),
            &AnySocketAddr::Unix(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Cannot get port from unix domain socket",
            )),
        }
    }
}
