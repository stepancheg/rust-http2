use bytes::Bytes;
use client::client_conn::ClientToWriteMessage;
use common::sender::CommonSender;
use common::sender::SendError;
use common::window_size::StreamDead;
use error;
use futures::Poll;
use futures::Stream;
use DataOrTrailers;
use ErrorCode;
use Headers;
use HttpStreamAfterHeaders;

pub struct ClientSender {
    pub(crate) common: CommonSender<ClientToWriteMessage>,
}

impl ClientSender {
    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        self.common.poll()
    }

    pub fn wait(&mut self) -> Result<(), StreamDead> {
        self.common.wait()
    }

    pub fn send_data(&mut self, data: Bytes) -> Result<(), SendError> {
        // TODO: check state
        self.common.send_data(data)
    }

    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> Result<(), SendError> {
        self.common.send_data_end_of_stream(data)
    }

    pub fn send_trailers(&mut self, trailers: Headers) -> Result<(), SendError> {
        self.common.send_trailers(trailers)
    }

    pub fn pull_from_stream(&mut self, stream: HttpStreamAfterHeaders) -> Result<(), SendError> {
        self.common.pull_from_stream(stream)
    }

    pub fn pull_bytes_from_stream<S>(&mut self, stream: S) -> Result<(), SendError>
    where
        S: Stream<Item = Bytes, Error = error::Error> + Send + 'static,
    {
        self.common.pull_bytes_from_stream(stream)
    }

    pub fn send_data_or_trailers(
        &mut self,
        data_or_trailers: DataOrTrailers,
    ) -> Result<(), SendError> {
        self.common.send_data_or_trailers(data_or_trailers)
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> Result<(), SendError> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> Result<(), SendError> {
        self.common.close()
    }
}
