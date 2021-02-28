use std::mem;
use std::task::Poll;

use bytes::Bytes;
use futures::stream::Stream;
use futures::task::Context;

use crate::assert_types::assert_send;
use crate::common::sender::CommonSender;
use crate::common::sender::SendError;
use crate::common::sink_after_headers::SinkAfterHeaders;
use crate::common::sink_after_headers::SinkAfterHeadersBox;
use crate::server::types::ServerTypes;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use crate::SenderState;
use crate::SimpleHttpMessage;

// NOTE: Keep in sync with ClientRequest
pub struct ServerResponse {
    pub(crate) common: CommonSender<ServerTypes>,
    // need to replace with FnOnce when rust allows it
    pub(crate) drop_callback: Option<Box<dyn FnOnce() -> crate::Result<SimpleHttpMessage> + Send>>,
}

impl Drop for ServerResponse {
    fn drop(&mut self) {
        if self.state() != SenderState::Done {
            warn!(
                "sender was not properly finished, state: {:?}, invoking custom callback",
                self.state()
            );
            if let Some(drop_callback) = mem::replace(&mut self.drop_callback, None) {
                match drop_callback() {
                    Err(e) => {
                        warn!("custom callback resulted in error: {}", e);
                    }
                    Ok(message) => {
                        let _ = self.send_message(message);
                    }
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
    where
        F: FnOnce() -> crate::Result<SimpleHttpMessage> + Send + 'static,
    {
        self.drop_callback = Some(Box::new(f));
    }

    pub fn clear_drop_callback(&mut self) {
        self.drop_callback = None;
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        self.common.poll(cx)
    }

    pub fn send_headers_new(self, _headers: Headers) -> SinkAfterHeadersBox {
        unimplemented!()
    }

    pub fn send_headers(&mut self, headers: Headers) -> Result<(), SendError> {
        self.common.send_headers(headers)
    }

    pub fn send_headers_end_of_stream(&mut self, headers: Headers) -> Result<(), SendError> {
        self.common.send_headers_end_of_stream(headers)
    }

    pub fn send_data(&mut self, data: Bytes) -> crate::Result<()> {
        self.common.send_data(data)
    }

    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> crate::Result<()> {
        self.common.send_data_end_of_stream(data)
    }

    pub fn send_trailers(&mut self, trailers: Headers) -> crate::Result<()> {
        self.common.send_trailers(trailers)
    }

    pub fn pull_from_stream<S: Stream<Item = crate::Result<DataOrTrailers>> + Send + 'static>(
        &mut self,
        stream: S,
    ) -> crate::Result<()> {
        self.common.pull_from_stream(stream)
    }

    pub fn pull_bytes_from_stream<S>(&mut self, stream: S) -> crate::Result<()>
    where
        S: Stream<Item = crate::Result<Bytes>> + Send + 'static,
    {
        self.common.pull_bytes_from_stream(stream)
    }

    pub fn send_message(&mut self, message: SimpleHttpMessage) -> crate::Result<()> {
        self.send_headers(message.headers)?;
        self.send_data_end_of_stream(message.body.into_bytes())?;
        Ok(())
    }

    pub fn send_found_200_plain_text(&mut self, body: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::found_200_plain_text(body))
    }

    pub fn send_redirect_302(&mut self, location: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::redirect_302(location))
    }

    pub fn send_not_found_404(&mut self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::not_found_404(message))
    }

    pub fn send_internal_error_500(&mut self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::internal_error_500(message))
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> crate::Result<()> {
        self.common.close()
    }
}
