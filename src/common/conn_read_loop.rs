use tokio_io::io::ReadHalf;

use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use codec::http_framed_read::HttpFramedJoinContinuationRead;
use rc_mut::RcMut;
use solicit::connection::HttpFrame;
use error;
use futures::Poll;
use futures::Async;


pub struct ReadLoop<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub framed_read: HttpFramedJoinContinuationRead<ReadHalf<T::Io>>,
    pub inner: RcMut<ConnData<T>>,
}

pub trait ReadLoopCustom {
    type Types : Types;
}

impl<T> ReadLoop<T>
    where
        T : Types,
        Self : ReadLoopCustom<Types=T>,
        ConnData<T> : ConnInner<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(&mut self) -> Poll<HttpFrame, error::Error> {
        let max_frame_size = self.inner.with(|inner| {
            inner.conn.our_settings_ack.max_frame_size
        });

        self.framed_read.poll_http_frame(max_frame_size)
    }

    /// Loop forever, never return `Ready`
    pub fn read_process_frame(&mut self) -> Poll<(), error::Error> {
        loop {
            if self.inner.with(|inner| inner.end_loop()) {
                return Err(error::Error::Other("GOAWAY"));
            }

            let frame = match self.recv_http_frame()? {
                Async::Ready(frame) => frame,
                Async::NotReady => return Ok(Async::NotReady),
            };

            self.process_http_frame(frame)?;
        }
    }

    fn process_http_frame(&mut self, frame: HttpFrame) -> Result<(), error::Error> {
        let inner_rc = self.inner.clone();

        self.inner.with(move |inner| {
            inner.process_http_frame(inner_rc, frame)
        })
    }
}
