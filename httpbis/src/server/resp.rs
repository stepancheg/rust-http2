use std::mem;

use crate::assert_types::assert_send;
use crate::common::sender::CommonSender;
use crate::common::sink_after_headers::SinkAfterHeaders;
use crate::common::sink_after_headers::SinkAfterHeadersBox;
use crate::server::types::ServerTypes;
use crate::ErrorCode;
use crate::Headers;
use crate::SimpleHttpMessage;

/// Server response provided to a server callback.
// NOTE: Keep in sync with ClientRequest
pub struct ServerResponse {
    pub(crate) common: Option<CommonSender<ServerTypes>>,
    // need to replace with FnOnce when rust allows it
    pub(crate) drop_callback: Option<Box<dyn FnOnce() -> crate::Result<SimpleHttpMessage> + Send>>,
}

impl Drop for ServerResponse {
    fn drop(&mut self) {
        if let Some(mut common) = self.common.take() {
            if let Some(drop_callback) = mem::replace(&mut self.drop_callback, None) {
                warn!("sender was not properly finished, invoking custom callback");
                match drop_callback() {
                    Err(e) => {
                        warn!("custom callback resulted in error: {}", e);
                    }
                    Ok(message) => {
                        let _ = common.send_headers(message.headers);
                        let _ = common.send_data_end_of_stream(message.body);
                    }
                }
            } else {
                // TODO: send 500?
                // warn!("sender was not properly finished calling reset");
                // let _ = common.send_headers_end_of_stream(Headers::internal_error_500());
            }
        }
    }
}

fn _assert_types() {
    assert_send::<ServerResponse>();
}

impl ServerResponse {
    /// Callback to be invoked on server drop.
    ///
    /// Callback is invoked if a request handler did not send headers first.
    pub fn set_drop_callback<F>(&mut self, f: F)
    where
        F: FnOnce() -> crate::Result<SimpleHttpMessage> + Send + 'static,
    {
        self.drop_callback = Some(Box::new(f));
    }

    /// Send response headers and return a sink which can be used
    /// to send response body and trailers.
    pub fn send_headers(mut self, headers: Headers) -> crate::Result<SinkAfterHeadersBox> {
        let mut common_sender = self.common.take().unwrap();
        common_sender.send_headers(headers)?;
        Ok(Box::pin(common_sender))
    }

    /// Send response headers and close the stream.
    ///
    /// Can be used to send redirect for example.
    pub fn send_headers_end_of_stream(mut self, headers: Headers) -> crate::Result<()> {
        let mut common_sender = self.common.take().unwrap();
        common_sender.send_headers_end_of_stream(headers)
    }

    /// Send headers, then send the message body, then close the response.
    pub fn send_message(self, message: SimpleHttpMessage) -> crate::Result<()> {
        let mut sink = self.send_headers(message.headers)?;
        sink.send_data_end_of_stream(message.body)?;
        Ok(())
    }

    /// Send plain text 200 response.
    pub fn send_found_200_plain_text(self, body: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::found_200_plain_text(body))
    }

    /// Send 302 redirect.
    pub fn send_redirect_302(self, location: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::redirect_302(location))
    }

    /// Send 404 response.
    pub fn send_not_found_404(self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::not_found_404(message))
    }

    /// Send internal error.
    pub fn send_internal_error_500(self, message: &str) -> crate::Result<()> {
        self.send_message(SimpleHttpMessage::internal_error_500(message))
    }

    /// Reset the stream (send `RST_STREAM` message).
    pub fn reset(mut self, error_code: ErrorCode) -> crate::Result<()> {
        self.common.take().unwrap().reset(error_code)
    }
}
