use futures::future::Future;
use tokio_io::AsyncWrite;
use tokio_io::io::write_all;
use error;
use solicit::connection::HttpFrame;
use solicit::frame::FrameIR;


pub struct HttpFramedWrite<W : AsyncWrite> {
    write: W,
}

impl<W : AsyncWrite> HttpFramedWrite<W> {
    pub fn new(write: W) -> Self {
        HttpFramedWrite {
            write,
        }
    }

    pub fn write_all(self, buf: Vec<u8>) -> impl Future<Item=Self, Error=error::Error> {
        let HttpFramedWrite { write } = self;
        write_all(write, buf)
            .map(|(write, _)| HttpFramedWrite { write })
            .map_err(error::Error::from)
    }

    pub fn write_frame(self, frame: HttpFrame) -> impl Future<Item=Self, Error=error::Error> {
        debug!("send {:?}", frame);
        
        self.write_all(frame.serialize_into_vec())
    }
}
