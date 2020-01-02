use std::any::Any;
use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio::net::UnixStream;

use futures::stream;

use futures::stream::Stream;
use futures::Future;

use crate::assert_types::assert_send_stream;
use crate::socket::AnySocketAddr;
use crate::socket::StreamItem;
use crate::socket::ToClientStream;
use crate::socket::ToServerStream;
use crate::socket::ToSocketListener;
use crate::socket::ToTokioListener;
use crate::ServerConf;
use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::runtime::Handle;

#[derive(Debug, Clone)]
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

impl ToSocketListener for SocketAddrUnix {
    #[cfg(unix)]
    fn to_listener(&self, _conf: &ServerConf) -> io::Result<Box<dyn ToTokioListener + Send>> {
        debug!("binding socket to {}", self);
        Ok(Box::new(::std::os::unix::net::UnixListener::bind(&self.0)?))
    }

    #[cfg(not(unix))]
    fn to_listener(&self, _conf: &ServerConf) -> io::Result<Box<ToTokioListener + Send>> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "cannot use unix sockets on non-unix",
        ))
    }

    fn cleanup(&self) {
        if self.0.exists() {
            fs::remove_file(&self.0).expect("removing socket during shutdown");
        }
    }
}

#[cfg(unix)]
impl ToTokioListener for ::std::os::unix::net::UnixListener {
    fn to_tokio_listener(self: Box<Self>, handle: &Handle) -> Box<dyn ToServerStream> {
        handle.enter(|| Box::new(UnixListener::from_std(*self).unwrap()))
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        let addr = self.local_addr().unwrap();
        let path = addr.as_pathname().unwrap();
        Ok(AnySocketAddr::Unix(SocketAddrUnix::from(path)))
    }
}

#[cfg(unix)]
impl ToServerStream for UnixListener {
    fn incoming(
        self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Stream<Item = io::Result<(Pin<Box<dyn StreamItem + Send>>, Box<dyn Any + Send>)>>
                + Send,
        >,
    > {
        let unix_listener = *self;

        let stream = stream::unfold(unix_listener, |mut unix_listener_listener| async {
            let r = match unix_listener_listener.accept().await {
                Ok((socket, addr)) => Ok((
                    Box::pin(socket) as Pin<Box<dyn StreamItem + Send>>,
                    Box::new(addr) as Box<dyn Any + Send>,
                )),
                Err(e) => Err(e),
            };
            Some((r, unix_listener_listener))
        });

        let stream = assert_send_stream::<
            io::Result<(Pin<Box<dyn StreamItem + Send>>, Box<dyn Any + Send>)>,
            _,
        >(stream);

        Box::pin(stream)
    }
}

impl ToClientStream for SocketAddrUnix {
    #[cfg(unix)]
    fn connect(
        &self,
        handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Pin<Box<dyn StreamItem + Send>>>> + Send>> {
        // TODO: async connect
        let stream = match std::os::unix::net::UnixStream::connect(&self.0) {
            Ok(stream) => stream,
            Err(e) => return Box::pin(async { Err(e) }),
        };
        match handle.enter(|| UnixStream::from_std(stream)) {
            Ok(stream) => {
                Box::pin(async { Ok(Box::pin(stream) as Pin<Box<dyn StreamItem + Send>>) })
            }
            Err(e) => return Box::pin(async { Err(e) }),
        }
    }

    #[cfg(not(unix))]
    fn connect(
        &self,
        _handle: &Handle,
    ) -> Pin<Box<dyn Future<Output = io::Result<Box<dyn StreamItem + Send>>> + Send>> {
        use futures::future;
        Box::pin(future::err(io::Error::new(
            io::ErrorKind::Other,
            "cannot use unix sockets on non-unix",
        )))
    }
}

#[cfg(unix)]
impl StreamItem for UnixStream {
    fn is_tcp(&self) -> bool {
        false
    }

    fn set_nodelay(&self, _no_delay: bool) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Cannot set nodelay on unix domain socket",
        ))
    }
}
