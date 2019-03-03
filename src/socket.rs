use std::any::Any;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::net::SocketAddr;

use tokio_core::reactor;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;

use futures::stream::Stream;
use futures::Future;
use socket_unix::SocketAddrUnix;
use ServerConf;

pub trait ToSocketListener {
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<ToTokioListener + Send>>;

    fn cleanup(&self);
}

#[derive(Clone, Debug)]
pub enum AnySocketAddr {
    Inet(SocketAddr),
    Unix(SocketAddrUnix),
}

impl Display for AnySocketAddr {
    fn fmt(&self, f: &mut Formatter) -> ::std::fmt::Result {
        match *self {
            AnySocketAddr::Inet(ref inet_addr) => Display::fmt(inet_addr, f),
            AnySocketAddr::Unix(ref unix_addr) => Display::fmt(unix_addr, f),
        }
    }
}

impl AnySocketAddr {
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

impl ToSocketListener for AnySocketAddr {
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<ToTokioListener + Send>> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.to_listener(conf),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.to_listener(conf),
        }
    }

    fn cleanup(&self) {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.cleanup(),
            #[cfg(unix)]
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.cleanup(),
            #[cfg(not(unix))]
            &AnySocketAddr::Unix(..) => {}
        }
    }
}

impl ToClientStream for AnySocketAddr {
    fn connect(
        &self,
        handle: &reactor::Handle,
    ) -> Box<Future<Item = Box<StreamItem>, Error = io::Error> + Send> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.connect(handle),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.connect(handle),
        }
    }
}

pub trait ToTokioListener {
    fn to_tokio_listener(self: Box<Self>, handle: &reactor::Handle) -> Box<ToServerStream>;

    fn local_addr(&self) -> io::Result<AnySocketAddr>;
}

pub trait ToServerStream {
    fn incoming(
        self: Box<Self>,
    ) -> Box<Stream<Item = (Box<StreamItem>, Box<Any>), Error = io::Error>>;
}

pub trait ToClientStream: Display + Send + Sync {
    fn connect(
        &self,
        handle: &reactor::Handle,
    ) -> Box<Future<Item = Box<StreamItem>, Error = io::Error> + Send>;
}

pub trait StreamItem: AsyncRead + AsyncWrite + io::Read + io::Write + Debug + Send + Sync {
    fn is_tcp(&self) -> bool;

    fn set_nodelay(&self, no_delay: bool) -> io::Result<()>;
}
