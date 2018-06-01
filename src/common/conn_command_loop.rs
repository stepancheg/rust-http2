use common::types::Types;
use common::conn::ConnData;
use common::conn::ConnInner;
use common::stream::HttpStreamCommon;
use common::stream::HttpStreamData;
use rc_mut::RcMut;
use solicit_async::HttpFutureStreamSend;


pub struct CommandLoop<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
    pub inner: RcMut<ConnData<T>>,
    pub requests: HttpFutureStreamSend<T::CommandMessage>,
}

impl<T> CommandLoop<T>
    where
        T : Types,
        ConnData<T> : ConnInner,
        HttpStreamCommon<T> : HttpStreamData,
{
}
