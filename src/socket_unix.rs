use std::io;
use std::any::Any;
use std::fs;
use std::path::Path;

use tokio_core::reactor;
use tokio_uds::UnixListener;
use tokio_uds::UnixStream;

use futures::stream::Stream;
use futures::Future;
use futures::future::ok;
use futures::future::err;

use socket::AnySocketAddr;
use socket::ToSocketListener;
use socket::ToTokioListener;
use socket::ToServerStream;
use socket::ToClientStream;
use socket::StreamItem;

use server_conf::ServerConf;


impl ToSocketListener for String {
    fn to_listener(&self, _conf: &ServerConf) -> Box<ToTokioListener + Send> {
        Box::new(::std::os::unix::net::UnixListener::bind(self).unwrap())
    }

    fn cleanup(&self) {
        if Path::new(self.as_str()).exists() {
            fs::remove_file(self.as_str()).expect("removing socket during shutdown");
        }
    }
}

impl ToTokioListener for ::std::os::unix::net::UnixListener {
    fn to_tokio_listener(self: Box<Self>, handle: &reactor::Handle) -> Box<ToServerStream> {
        Box::new(UnixListener::from_listener(*self, handle).unwrap())
    }

    fn local_addr(&self) -> io::Result<AnySocketAddr> {
        let addr = self.local_addr().unwrap();
        let path = addr.as_pathname().unwrap();
        let string = path.to_str().unwrap().to_owned();

        Ok(AnySocketAddr::Unix(string))
    }
}

impl ToServerStream for UnixListener {
    fn incoming(self: Box<Self>)
        -> Box<Stream<Item=(Box<StreamItem>, Box<Any>), Error=io::Error>>
    {
        let stream = (*self).incoming().map(|(stream, addr)|
            (Box::new(stream) as Box<StreamItem>, Box::new(addr) as Box<Any>)
        );
        Box::new(stream)
    }
}

impl ToClientStream for String {
    fn connect(&self, handle: &reactor::Handle)
        -> Box<Future<Item=Box<StreamItem>, Error=io::Error> + Send>
    {
        let stream = UnixStream::connect(Path::new(self), &handle);
        if stream.is_ok() {
            Box::new(ok(Box::new(stream.unwrap()) as Box<StreamItem>))
        } else {
            Box::new(err(stream.unwrap_err()))
        }
    }
}

impl StreamItem for UnixStream {
    fn is_tcp(&self) -> bool {
        false
    }

    fn set_nodelay(&self, _no_delay: bool) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Other, "Cannot set nodelay on unix domain socket"))
    }
}
