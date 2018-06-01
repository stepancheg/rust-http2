use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use rc_mut::RcMut;
use solicit_async::HttpFutureStreamSend;
use result;
use futures::Poll;
use error;
use futures::Async;
use futures::Future;
use futures::future;


pub struct CommandLoop<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub inner: RcMut<ConnData<T>>,
    pub requests: HttpFutureStreamSend<T::CommandMessage>,
}

pub trait CommandLoopCustom {
    type Types : Types;

    fn process_command_message(&mut self, message: <Self::Types as Types>::CommandMessage)
        -> result::Result<()>;
}

impl<T> CommandLoop<T>
    where
        T : Types,
        Self : CommandLoopCustom<Types=T>,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    fn poll_command(&mut self)
        -> Poll<(), error::Error>
    {
        loop {
            let message = match self.requests.poll()? {
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(Some(message)) => message,
                Async::Ready(None) => return Ok(Async::Ready(())),
            };

            self.process_command_message(message)?;
        }
    }

    pub fn run_command(mut self)
        -> Box<Future<Item=(), Error=error::Error>>
    {
        Box::new(future::poll_fn(move || self.poll_command()))
    }
}
