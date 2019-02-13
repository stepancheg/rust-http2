use bytes::Bytes;
use common::sender::CommonSender;
use common::sender::SendError;
use error;
use futures::Poll;
use futures::Stream;
use server::types::ServerTypes;
use ErrorCode;
use Headers;
use HttpStreamAfterHeaders;
use SenderState;
use SimpleHttpMessage;
use StreamDead;
use std::mem;
use result;
use assert_types::assert_send;


// NOTE: Keep in sync with ClientRequest
pub struct ServerResponse {
    pub(crate) common: CommonSender<ServerTypes>,
    // need to replace with FnOnce when rust allows it
    pub(crate) drop_callback: Option<Box<FnMut(&mut ServerResponse) -> result::Result<()> + Send>>,
}

impl Drop for ServerResponse {
    fn drop(&mut self) {
        if self.state() != SenderState::Done {
            warn!("sender was not properly finished, state: {:?}, invoking custom callback", self.state());
            if let Some(mut drop_callback) = mem::replace(&mut self.drop_callback, None) {
                if let Err(e) = drop_callback(self) {
                    warn!("custom callback resulted in error: {:?}", e);
                }
            }
        }
    }
}

fn _assert_types() {
    assert_send::<ServerResponse>();
}

impl ServerResponse {
    pub fn state(&self) -> SenderState {
        self.common.state()
    }

    pub fn set_drop_callback<F>(&mut self, f: F)
        where F: FnMut(&mut ServerResponse) -> result::Result<()> + Send + 'static
    {
        self.drop_callback = Some(Box::new(f));
    }

    pub fn clear_drop_callback(&mut self) {
        mem::replace(&mut self.drop_callback, None);
    }

    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        self.common.poll()
    }

    pub fn block_wait(&mut self) -> Result<(), StreamDead> {
        self.common.block_wait()
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

    pub fn send_internal_error_500(&mut self, message: &str) -> Result<(), SendError> {
        self.send_message(SimpleHttpMessage::internal_error_500(message))
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> Result<(), SendError> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> Result<(), SendError> {
        self.common.close()
    }
}
