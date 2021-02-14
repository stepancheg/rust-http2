use std::io;
use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio::net::TcpStream;

use futures::future::Future;
use futures::future::TryFutureExt;

use net2;

use crate::net::addr::AnySocketAddr;
use crate::net::connect::ToClientStream;
use crate::net::listen::SocketListener;
use crate::net::listen::ToSocketListener;
use crate::net::listen::ToTokioListener;
use crate::net::socket::SocketStream;
use crate::ServerConf;
use std::pin::Pin;
use tokio::runtime::Handle;

impl ToSocketListener for SocketAddr {
    fn listen(&self, conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        Ok(Box::new(listener(self, conf)?))
    }

    fn cleanup(&self) {}
}

#[cfg(not(windows))]
fn configure_tcp(tcp: &net2::TcpBuilder, conf: &ServerConf) -> io::Result<()> {
    use net2::unix::UnixTcpBuilderExt;
    if let Some(reuse_port) = conf.reuse_port {
        tcp.reuse_port(reuse_port)?;
    }
    Ok(())
}

#[cfg(windows)]
fn configure_tcp(_tcp: &net2::TcpBuilder, _conf: &ServerConf) -> io::Result<()> {
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
    fn into_tokio_listener(self: Box<Self>, handle: &Handle) -> Pin<Box<dyn SocketListener>> {
        let _g = handle.enter();
        Box::pin(TcpListener::from_std(*self).unwrap())
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        Ok(AnySocketAddr::Inet(self.local_addr().unwrap()))
    }
}

impl SocketListener for TcpListener {
    fn accept<'a>(
        self: Pin<&'a mut Self>,
    ) -> Pin<
        Box<
            dyn Future<Output = io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let (socket, peer_addr) = TcpListener::accept(Pin::get_mut(self)).await?;
            Ok((
                Box::pin(socket) as Pin<Box<dyn SocketStream + Send>>,
                AnySocketAddr::Inet(peer_addr),
            ))
        })
    }
}

impl ToClientStream for SocketAddr {
    fn connect(
        &self,
        _handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream>>>> + Send>> {
        let future = TcpStream::connect(self.clone())
            .map_ok(|stream| Box::pin(stream) as Pin<Box<dyn SocketStream>>);
        Box::pin(future)
    }

    fn socket_addr(&self) -> AnySocketAddr {
        AnySocketAddr::Inet(*self)
    }
}

impl SocketStream for TcpStream {
    fn is_tcp(&self) -> bool {
        true
    }

    fn set_tcp_nodelay(&self, no_delay: bool) -> io::Result<()> {
        self.set_nodelay(no_delay)
    }

    fn peer_addr(&self) -> io::Result<AnySocketAddr> {
        Ok(AnySocketAddr::Inet(TcpStream::peer_addr(self)?))
    }
}
