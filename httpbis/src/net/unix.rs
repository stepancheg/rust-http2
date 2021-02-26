use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio::net::UnixStream;

use futures::Future;

use crate::net::addr::AnySocketAddr;
use crate::net::connect::ToClientStream;
use crate::net::listen::SocketListener;
use crate::net::listen::ToSocketListener;
use crate::net::listen::ToTokioListener;
use crate::net::socket::SocketStream;
use crate::ServerConf;
use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::runtime::Handle;

/// Unix socket address, which is filesystem path.
///
/// Note although this type is available on Windows, unix sockets don't work on Windows.
#[derive(Debug, Clone, PartialEq)]
pub struct SocketAddrUnix(pub(crate) PathBuf);

impl From<PathBuf> for SocketAddrUnix {
    fn from(p: PathBuf) -> Self {
        SocketAddrUnix(p)
    }
}

impl From<&Path> for SocketAddrUnix {
    fn from(p: &Path) -> Self {
        SocketAddrUnix(p.into())
    }
}

impl From<&str> for SocketAddrUnix {
    fn from(p: &str) -> Self {
        SocketAddrUnix(p.into())
    }
}

impl From<&String> for SocketAddrUnix {
    fn from(p: &String) -> Self {
        SocketAddrUnix(p.into())
    }
}

impl From<String> for SocketAddrUnix {
    fn from(p: String) -> Self {
        SocketAddrUnix(p.into())
    }
}

impl fmt::Display for SocketAddrUnix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0.display(), f)
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::SocketAddr> for SocketAddrUnix {
    fn from(s: std::os::unix::net::SocketAddr) -> Self {
        // can be unnamed
        SocketAddrUnix(s.as_pathname().unwrap_or(Path::new("")).to_owned())
    }
}

#[cfg(unix)]
impl From<tokio::net::unix::SocketAddr> for SocketAddrUnix {
    fn from(s: tokio::net::unix::SocketAddr) -> Self {
        // can be unnamed
        SocketAddrUnix(s.as_pathname().unwrap_or(Path::new("")).to_owned())
    }
}

impl ToSocketListener for SocketAddrUnix {
    #[cfg(unix)]
    fn listen(&self, _conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        debug!("binding socket to {}", self);
        Ok(Box::new(::std::os::unix::net::UnixListener::bind(&self.0)?))
    }

    #[cfg(not(unix))]
    fn listen(&self, _conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "cannot use unix sockets on non-unix",
        ))
    }

    fn cleanup(&self) {
        if self.0.exists() {
            // TODO: do not panic
            fs::remove_file(&self.0).expect("removing socket during shutdown");
        }
    }
}

#[cfg(unix)]
impl ToTokioListener for ::std::os::unix::net::UnixListener {
    fn into_tokio_listener(self: Box<Self>, handle: &Handle) -> Pin<Box<dyn SocketListener>> {
        let _g = handle.enter();
        self.set_nonblocking(true).unwrap();
        Box::pin(UnixListener::from_std(*self).unwrap())
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        let addr = self.local_addr().unwrap();
        let path = addr.as_pathname().unwrap();
        Ok(AnySocketAddr::Unix(SocketAddrUnix::from(path)))
    }
}

#[cfg(unix)]
impl SocketListener for UnixListener {
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
            let (socket, peer_addr) = UnixListener::accept(Pin::get_mut(self)).await?;
            Ok((
                Box::pin(socket) as Pin<Box<dyn SocketStream + Send>>,
                AnySocketAddr::Unix(peer_addr.into()),
            ))
        })
    }
}

impl ToClientStream for SocketAddrUnix {
    #[cfg(unix)]
    fn connect<'a>(
        &'a self,
        handle: &Handle,
    ) -> Pin<
        Box<
            dyn Future<Output = io::Result<(AnySocketAddr, Pin<Box<dyn SocketStream>>)>>
                + Send
                + 'a,
        >,
    > {
        let _g = handle.enter();
        let addr = AnySocketAddr::Unix(self.clone());
        Box::pin(async move {
            let stream = UnixStream::connect(&self.0).await?;
            let stream: Pin<Box<dyn SocketStream>> = Box::pin(stream);
            Ok((addr, stream))
        })
    }

    #[cfg(not(unix))]
    fn connect<'a>(
        &'a self,
        _handle: &Handle,
    ) -> Pin<
        Box<
            dyn Future<Output = io::Result<(AnySocketAddr, Pin<Box<dyn SocketStream>>)>>
                + Send
                + 'a,
        >,
    > {
        use futures::future;
        Box::pin(future::err(io::Error::new(
            io::ErrorKind::Other,
            "cannot use unix sockets on non-unix",
        )))
    }
}

#[cfg(unix)]
impl SocketStream for UnixStream {
    fn is_tcp(&self) -> bool {
        false
    }

    fn set_tcp_nodelay(&self, _no_delay: bool) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Cannot set nodelay on unix domain socket",
        ))
    }

    fn peer_addr(&self) -> io::Result<AnySocketAddr> {
        Ok(AnySocketAddr::from(UnixStream::peer_addr(self)?))
    }
}

#[cfg(test)]
mod test {
    use crate::net::connect::ToClientStream;
    use crate::net::unix::SocketAddrUnix;
    use crate::AnySocketAddr;
    use std::path::PathBuf;
    use tokio::runtime::Runtime;

    #[cfg(unix)]
    #[test]
    fn peer_addr() {
        let lp = Runtime::new().unwrap();
        let h = lp.handle().clone();

        let dir = tempdir::TempDir::new("peer_addr").unwrap();
        let p = format!("{}/s", dir.path().display());
        let _server = std::os::unix::net::UnixListener::bind(&p).unwrap();

        let client = lp
            .block_on(async { SocketAddrUnix(PathBuf::from(&p)).connect(&h).await.unwrap() })
            .1;

        assert_eq!(
            AnySocketAddr::Unix(SocketAddrUnix::from(&p)),
            client.peer_addr().unwrap()
        );
    }
}
