use std::any::Any;
use std::io;
use std::net::SocketAddr;

use tokio_core::net::TcpListener;
use tokio_core::net::TcpStream;
use tokio_core::reactor;

use futures::stream::Stream;
use futures::Future;

use net2;

use socket::AnySocketAddr;
use socket::StreamItem;
use socket::ToClientStream;
use socket::ToServerStream;
use socket::ToSocketListener;
use socket::ToTokioListener;
use ServerConf;

impl ToSocketListener for SocketAddr {
    fn to_listener(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        Ok(Box::new(listener(self, conf)?))
    }

    fn cleanup(&self) {}
}

#[cfg(unix)]
fn configure_tcp(tcp: &net2::TcpBuilder, conf: &ServerConf) -> io::Result<()> {
    use net2::unix::UnixTcpBuilderExt;
    if let Some(reuse_port) = conf.reuse_port {
        tcp.reuse_port(reuse_port)?;
    }
    Ok(())
}

#[cfg(windows)]
fn configure_tcp(_tcp: &net2::TcpBuilder, conf: &ServerConf) -> io::Result<()> {
    Ok(())
}

fn listener(addr: &SocketAddr, conf: &ServerConf) -> io::Result<::std::net::TcpListener> {
    let listener = match *addr {
        SocketAddr::V4(_) => net2::TcpBuilder::new_v4()?,
        SocketAddr::V6(_) => net2::TcpBuilder::new_v6()?,
    };

    if let SocketAddr::V6(_) = *addr {
        // Assume only_v6 = false by default
        // Note: it is not default behavior for Rust
        let only_v6 = conf.only_v6.unwrap_or(false);
        listener.only_v6(only_v6)?;
    }

    configure_tcp(&listener, conf)?;
    listener.reuse_address(true)?;
    debug!("binding socket to {}", addr);
    listener.bind(addr)?;
    let backlog = conf.backlog.unwrap_or(1024);
    listener.listen(backlog)
}

impl ToTokioListener for ::std::net::TcpListener {
    fn to_tokio_listener(self: Box<Self>, handle: &reactor::Handle) -> Box<dyn ToServerStream> {
        let local_addr = self.local_addr().unwrap();
        Box::new(TcpListener::from_listener(*self, &local_addr, handle).unwrap())
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        Ok(AnySocketAddr::Inet(self.local_addr().unwrap()))
    }
}

impl ToServerStream for TcpListener {
    fn incoming(
        self: Box<Self>,
    ) -> Box<dyn Stream<Item = (Box<dyn StreamItem>, Box<dyn Any>), Error = io::Error>> {
        let stream = (*self).incoming().map(|(stream, addr)| {
            (
                Box::new(stream) as Box<dyn StreamItem>,
                Box::new(addr) as Box<dyn Any>,
            )
        });
        Box::new(stream)
    }
}

impl ToClientStream for SocketAddr {
    fn connect(
        &self,
        handle: &reactor::Handle,
    ) -> Box<dyn Future<Item = Box<dyn StreamItem>, Error = io::Error> + Send> {
        let stream =
            TcpStream::connect(self, &handle).map(|stream| Box::new(stream) as Box<dyn StreamItem>);
        Box::new(stream)
    }
}

impl StreamItem for TcpStream {
    fn is_tcp(&self) -> bool {
        true
    }

    fn set_nodelay(&self, no_delay: bool) -> io::Result<()> {
        self.set_nodelay(no_delay)
    }
}
