use common::types::Types;
use common::conn::ConnData;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use result;
use futures::Poll;
use error;
use futures::Async;


pub trait CommandLoopCustom {
    type Types : Types;

    fn process_command_message(&mut self, message: <Self::Types as Types>::CommandMessage)
        -> result::Result<()>;
}

impl<T> ConnData<T>
    where
        T : Types,
        Self : CommandLoopCustom<Types=T>,
        HttpStreamCommon<T> : HttpStreamData<Types=T>,
{
    pub fn poll_command(&mut self)
        -> Poll<(), error::Error>
    {
        loop {
            let message = match self.command_rx.poll()? {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(Some(message)) => message,
                Async::Ready(None) => return Ok(Async::Ready(())),
            };

            self.process_command_message(message)?;
        }
    }
}
