use crate::net::unix::SocketAddrUnix;
use std::fmt;
use std::io;
use std::net::SocketAddr;

// Any socket address: TCP or Unix
#[derive(Clone, Debug, PartialEq)]
pub enum AnySocketAddr {
    Inet(SocketAddr),
    Unix(SocketAddrUnix),
}

impl From<SocketAddr> for AnySocketAddr {
    fn from(inet: SocketAddr) -> Self {
        AnySocketAddr::Inet(inet)
    }
}

impl From<SocketAddrUnix> for AnySocketAddr {
    fn from(unix: SocketAddrUnix) -> Self {
        AnySocketAddr::Unix(unix)
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::SocketAddr> for AnySocketAddr {
    fn from(unix: std::os::unix::net::SocketAddr) -> Self {
        AnySocketAddr::Unix(SocketAddrUnix::from(unix))
    }
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
