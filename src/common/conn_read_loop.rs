use futures::future;
use futures::future::Loop;
use futures::future::Future;
use futures::future::loop_fn;

use tokio_io::AsyncRead;
use tokio_io::io::ReadHalf;

use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use codec::http_frame_read::HttpFrameJoinContinuationRead;
use rc_mut::RcMut;
use solicit::connection::HttpFrame;
use error;
use solicit_async::HttpFuture;


pub struct ReadLoop<I, T>
    where
        I : AsyncRead + 'static,
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub framed_read: HttpFrameJoinContinuationRead<ReadHalf<I>>,
    pub inner: RcMut<ConnData<T>>,
}

impl<I, T> ReadLoop<I, T>
    where
        I : AsyncRead + Send + 'static,
        T : Types,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(self) -> impl Future<Item=(Self, HttpFrame), Error=error::Error> {
        let ReadLoop { framed_read, inner } = self;

        let max_frame_size = inner.with(|inner| {
            inner.conn.our_settings_ack.max_frame_size
        });

        framed_read.recv_http_frame(max_frame_size)
            .map(|(framed_read, frame)| (ReadLoop { framed_read, inner }, frame))
    }

    fn read_process_frame(self) -> impl Future<Item=Self, Error=error::Error> {
        self.recv_http_frame()
            .and_then(move |(lp, frame)| lp.process_http_frame(frame))
    }

    fn loop_iter(self) -> HttpFuture<Loop<(), Self>> {
        if self.inner.with(|inner| inner.end_loop()) {
            return Box::new(future::err(error::Error::Other("GOAWAY")));
            //return Box::new(future::ok(Loop::Break(())));
        }

        Box::new(self.read_process_frame().map(Loop::Continue))
    }

    pub fn run(self) -> HttpFuture<()> {
        Box::new(loop_fn(self, Self::loop_iter))
    }

    fn process_http_frame(self, frame: HttpFrame) -> HttpFuture<Self> {
        let inner_rc = self.inner.clone();

        Box::new(future::result(self.inner.with(move |inner| {
            inner.process_http_frame(inner_rc, frame)
        }).map(|()| self)))
    }

}
