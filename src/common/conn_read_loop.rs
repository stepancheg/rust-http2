use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use solicit::connection::HttpFrame;
use error;
use futures::Poll;
use futures::Async;
use common::conn_write_loop::WriteLoopCustom;
use common::conn_command_loop::CommandLoopCustom;


pub trait ReadLoopCustom {
    type Types : Types;
}

impl<T> ConnData<T>
    where
        T : Types,
        Self : ConnInner<Types=T>,
        Self : ReadLoopCustom<Types=T>,
        Self : WriteLoopCustom<Types=T>,
        Self : CommandLoopCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    /// Recv a frame from the network
    fn recv_http_frame(&mut self) -> Poll<HttpFrame, error::Error> {
        let max_frame_size = self.conn.our_settings_ack.max_frame_size;

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
