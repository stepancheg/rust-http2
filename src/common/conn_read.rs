use common::types::Types;
use common::conn::Conn;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use solicit::connection::HttpFrame;
use solicit::StreamId;
use error;
use futures::Poll;
use futures::Async;
use common::conn_write::ConnWriteSideCustom;
use common::conn_command::ConnCommandSideCustom;
use solicit::connection::EndStream;
use Headers;
use result;
use common::stream_map::HttpStreamRef;


pub trait ConnReadSideCustom {
    type Types : Types;

    fn process_headers(&mut self, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<Self::Types>>>;
}

impl<T> Conn<T>
    where
        T : Types,
        Self : ConnReadSideCustom<Types=T>,
        Self : ConnWriteSideCustom<Types=T>,
        Self : ConnCommandSideCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(&mut self) -> Poll<HttpFrame, error::Error> {
        let max_frame_size = self.our_settings_ack.max_frame_size;

        self.framed_read.poll_http_frame(max_frame_size)
    }

    /// Loop forever, never return `Ready`
    pub fn read_process_frame(&mut self) -> Poll<(), error::Error> {
        loop {
            if self.end_loop() {
                return Err(error::Error::Other("GOAWAY"));
            }

            let frame = match self.recv_http_frame()? {
                Async::Ready(frame) => frame,
                Async::NotReady => return Ok(Async::NotReady),
            };

            self.process_http_frame(frame)?;
        }
    }
}
