use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;
use std::net::SocketAddr;

use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

use crate::socket_unix::SocketAddrUnix;
use crate::ServerConf;
use futures::stream::Stream;
use futures::Future;
use std::pin::Pin;
use tokio::runtime::Handle;

pub trait ToSocketListener {
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>>;

    fn cleanup(&self);
}

#[derive(Clone, Debug, PartialEq)]
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
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
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
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream + Send>>>> + Send>> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.connect(handle),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.connect(handle),
        }
    }

    fn socket_addr(&self) -> AnySocketAddr {
        self.clone()
    }
}

pub trait ToTokioListener {
    fn to_tokio_listener(self: Box<Self>, handle: &Handle) -> Box<dyn ToServerStream>;

    fn local_addr(&self) -> io::Result<AnySocketAddr>;
}

pub trait ToServerStream {
    fn incoming(
        self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Stream<Item = io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>>
                + Send,
        >,
    >;
}

pub trait ToClientStream: Display + Send + Sync {
    fn connect(
        &self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream + Send>>>> + Send>>;

    fn socket_addr(&self) -> AnySocketAddr;
}

pub trait SocketStream: AsyncRead + AsyncWrite + Debug + Send + Sync {
    fn is_tcp(&self) -> bool;

    fn set_nodelay(&self, no_delay: bool) -> io::Result<()>;
}
