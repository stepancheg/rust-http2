use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::time;

use crate::net::socket::SocketStream;
use crate::AnySocketAddr;
use futures::TryFutureExt;

pub trait ToClientStream: fmt::Display + Send + Sync + 'static {
    fn connect<'a>(
        &'a self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream>>>> + Send + 'a>>;

    fn socket_addr(&self) -> AnySocketAddr;
}

impl dyn ToClientStream {
    pub fn connect_with_timeout<'a>(
        &'a self,
        handle: &Handle,
        timeout: Option<Duration>,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Pin<Box<dyn SocketStream>>>> + Send + 'a>> {
        match timeout {
            Some(timeout) => {
                let connect = self.connect(handle);
                Box::pin(async move {
                    match time::timeout(timeout, connect).await {
                        Ok(r) => Ok(r?),
                        Err(_) => Err(crate::Error::ConnectionTimeout),
                    }
                })
            }
            None => Box::pin(self.connect(handle).map_err(crate::Error::from)),
        }
    }
}

impl ToClientStream for AnySocketAddr {
    fn connect<'a>(
        &'a self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream>>>> + Send + 'a>> {
        match self {
            &AnySocketAddr::Inet(ref inet_addr) => inet_addr.connect(handle),
            &AnySocketAddr::Unix(ref unix_addr) => unix_addr.connect(handle),
        }
    }

    fn socket_addr(&self) -> AnySocketAddr {
        self.clone()
    }
}
