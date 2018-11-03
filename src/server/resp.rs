use bytes::Bytes;
use common::sender::CommonSender;
use common::sender::SendError;
use error;
use futures::Poll;
use futures::Stream;
use server::server_conn::ServerToWriteMessage;
use DataOrTrailers;
use ErrorCode;
use Headers;
use HttpStreamAfterHeaders;
use SimpleHttpMessage;
use StreamDead;

pub struct ServerResponse {
    pub(crate) common: CommonSender<ServerToWriteMessage>,
}

impl ServerResponse {
    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        self.common.poll()
    }

    pub fn wait(&mut self) -> Result<(), StreamDead> {
        self.common.wait()
    }

    pub fn send_headers(&mut self, headers: Headers) -> Result<(), SendError> {
        self.common.send_headers(headers)
    }

    pub fn send_data(&mut self, data: Bytes) -> Result<(), SendError> {
        self.common.send_data(data)
    }

    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> Result<(), SendError> {
        self.common.send_data_end_of_stream(data)
    }

    pub fn send_trailers(&mut self, trailers: Headers) -> Result<(), SendError> {
        self.common.send_trailers(trailers)
    }

    pub fn send_data_or_trailers(
        &mut self,
        data_or_trailers: DataOrTrailers,
    ) -> Result<(), SendError> {
        self.common.send_data_or_trailers(data_or_trailers)
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

    pub fn send_message(&mut self, message: SimpleHttpMessage) -> Result<(), SendError> {
        self.send_headers(message.headers)?;
        self.send_data_end_of_stream(message.body)?;
        Ok(())
    }

    pub fn send_found_200_plain_text(&mut self, body: &str) -> Result<(), SendError> {
        self.send_message(SimpleHttpMessage::found_200_plain_text(body))
    }

    pub fn send_redirect_302(&mut self, location: &str) -> Result<(), SendError> {
        self.send_message(SimpleHttpMessage::redirect_302(location))
    }

    pub fn send_not_found_404(&mut self, message: &str) -> Result<(), SendError> {
        self.send_message(SimpleHttpMessage::not_found_404(message))
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> Result<(), SendError> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> Result<(), SendError> {
        self.common.close()
    }
}
