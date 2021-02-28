use std::mem;

use crate::assert_types::assert_send;
use crate::common::sender::CommonSender;
use crate::common::sender::SendError;
use crate::common::sink_after_headers::SinkAfterHeaders;
use crate::common::sink_after_headers::SinkAfterHeadersBox;
use crate::server::types::ServerTypes;
use crate::ErrorCode;
use crate::Headers;
use crate::SenderState;
use crate::SimpleHttpMessage;

// NOTE: Keep in sync with ClientRequest
pub struct ServerResponse {
    pub(crate) common: Option<CommonSender<ServerTypes>>,
    // need to replace with FnOnce when rust allows it
    pub(crate) drop_callback: Option<Box<dyn FnOnce() -> crate::Result<SimpleHttpMessage> + Send>>,
}

impl Drop for ServerResponse {
    fn drop(&mut self) {
        if let Some(mut common) = self.common.take() {
            warn!("sender was not properly finished, invoking custom callback if any",);
            if let Some(drop_callback) = mem::replace(&mut self.drop_callback, None) {
                match drop_callback() {
                    Err(e) => {
                        warn!("custom callback resulted in error: {}", e);
                    }
                    Ok(message) => {
                        let _ = common.send_headers(message.headers);
                        let _ = common.send_data_end_of_stream(message.body.get_bytes());
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
    pub fn set_drop_callback<F>(&mut self, f: F)
    where
        F: FnOnce() -> crate::Result<SimpleHttpMessage> + Send + 'static,
    {
        self.drop_callback = Some(Box::new(f));
    }

    pub fn clear_drop_callback(&mut self) {
        self.drop_callback = None;
    }

    pub fn send_headers_new(mut self, headers: Headers) -> crate::Result<SinkAfterHeadersBox> {
        match self.common.take() {
            None => Err(SendError::IncorrectState(SenderState::ExpectingBodyOrTrailers).into()),
            Some(mut common) => {
                common.send_headers(headers)?;
                Ok(Box::pin(common))
            }
        }
    }

    pub fn send_headers(self, headers: Headers) -> crate::Result<SinkAfterHeadersBox> {
        self.send_headers_new(headers)
    }

    pub fn send_headers_end_of_stream(&mut self, headers: Headers) -> crate::Result<()> {
        match self.common.take() {
            None => Err(SendError::IncorrectState(SenderState::ExpectingBodyOrTrailers).into()),
            Some(mut common) => {
                common.send_headers_end_of_stream(headers)?;
                Ok(())
            }
        }
    }

    pub fn send_message(self, message: SimpleHttpMessage) -> crate::Result<()> {
        let mut sink = self.send_headers(message.headers)?;
        sink.send_data_end_of_stream(message.body.into_bytes())?;
        Ok(())
    }

    pub fn send_found_200_plain_text(self, body: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::found_200_plain_text(body))
    }

    pub fn send_redirect_302(self, location: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::redirect_302(location))
    }

    pub fn send_not_found_404(self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::not_found_404(message))
    }

    pub fn send_internal_error_500(self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::internal_error_500(message))
    }

    pub fn reset(mut self, error_code: ErrorCode) -> crate::Result<()> {
        self.common.take().unwrap().reset(error_code)
    }
}
