use crate::net::socket::SocketStream;
use crate::{AnySocketAddr, ServerConf};
use futures::Stream;
use std::io;
use std::pin::Pin;
use tokio::runtime::Handle;

pub trait ToSocketListener {
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>>;

    fn cleanup(&self);
}

pub trait ToTokioListener {
    fn to_tokio_listener(self: Box<Self>, handle: &Handle) -> Box<dyn ToServerStream>;

    fn local_addr(&self) -> io::Result<AnySocketAddr>;
}

pub trait ToServerStream: Send {
    fn incoming(
        self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Stream<Item = io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>>
                + Send,
        >,
    >;
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
