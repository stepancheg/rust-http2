use crate::net::socket::SocketStream;
use crate::AnySocketAddr;
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use tokio::runtime::Handle;

pub trait ToClientStream: fmt::Display + Send + Sync {
    fn connect(
        &self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream>>>> + Send>>;

    fn socket_addr(&self) -> AnySocketAddr;
}

impl ToClientStream for AnySocketAddr {
    fn connect(
        &self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream>>>> + Send>> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.connect(handle),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.connect(handle),
        }
    }

    fn socket_addr(&self) -> AnySocketAddr {
        self.clone()
    }
}
