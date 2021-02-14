use crate::net::socket::SocketStream;
use crate::AnySocketAddr;
use crate::ServerConf;
use std::future::Future;
use std::io;
use std::pin::Pin;
use tokio::runtime::Handle;

/// Create a listener socket.
pub trait ToSocketListener {
    fn listen(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>>;

    fn cleanup(&self);
}

/// Convert a listening socket to tokio-compatible socket listener.
pub trait ToTokioListener {
    fn into_tokio_listener(self: Box<Self>, handle: &Handle) -> Pin<Box<dyn SocketListener>>;

    fn local_addr(&self) -> io::Result<AnySocketAddr>;
}

pub trait SocketListener: Send {
    fn accept<'a>(
        self: Pin<&'a mut Self>,
    ) -> Pin<
        Box<
            dyn Future<Output = io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>>
                + Send
                + 'a,
        >,
    >;
}

impl ToSocketListener for AnySocketAddr {
    fn listen(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.listen(conf),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.listen(conf),
        }
    }

    fn cleanup(&self) {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.cleanup(),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.cleanup(),
        }
    }
}
