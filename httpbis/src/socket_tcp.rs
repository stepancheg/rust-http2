use std::io;
use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio::net::TcpStream;

use futures::future::Future;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::Stream;

use net2;

use crate::assert_types::assert_send_stream;
use crate::socket::AnySocketAddr;
use crate::socket::SocketStream;
use crate::socket::ToClientStream;
use crate::socket::ToServerStream;
use crate::socket::ToSocketListener;
use crate::socket::ToTokioListener;
use crate::ServerConf;
use std::pin::Pin;
use tokio::runtime::Handle;

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
    fn to_tokio_listener(self: Box<Self>, handle: &Handle) -> Box<dyn ToServerStream> {
        handle.enter(|| Box::new(TcpListener::from_std(*self).unwrap()))
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        Ok(AnySocketAddr::Inet(self.local_addr().unwrap()))
    }
}

impl ToServerStream for TcpListener {
    fn incoming(
        self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Stream<Item = io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>>
                + Send,
        >,
    > {
        let tcp_listener = *self;

        let stream = stream::unfold(tcp_listener, |mut tcp_listener| async move {
            let r = match tcp_listener.accept().await {
                Ok((socket, addr)) => Ok((
                    Box::pin(socket) as Pin<Box<dyn SocketStream + Send>>,
                    AnySocketAddr::Inet(addr),
                )),
                Err(e) => Err(e),
            };
            Some((r, tcp_listener))
        });

        let stream = assert_send_stream::<
            io::Result<(Pin<Box<dyn SocketStream + Send>>, AnySocketAddr)>,
            _,
        >(stream);

        Box::pin(stream)
    }
}

impl ToClientStream for SocketAddr {
    fn connect(
        &self,
        _handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn SocketStream + Send>>>> + Send>> {
        let future = TcpStream::connect(self.clone())
            .map_ok(|stream| Box::pin(stream) as Pin<Box<dyn SocketStream + Send>>);
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

    fn set_nodelay(&self, no_delay: bool) -> io::Result<()> {
        self.set_nodelay(no_delay)
    }
}
