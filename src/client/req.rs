use assert_types::assert_send;
use bytes::Bytes;
use client::types::ClientTypes;
use common::sender::CommonSender;
use common::sender::SendError;
use common::window_size::StreamDead;
use error;
use futures::Poll;
use futures::Stream;
use result;
use std::mem;
use ErrorCode;
use Headers;
use HttpStreamAfterHeaders;
use SenderState;

/// Reference to outgoing stream on the client side.
// NOTE: keep in sync with ServerResponse
pub struct ClientRequest {
    pub(crate) common: CommonSender<ClientTypes>,
    // need to replace with FnOnce when rust allows it
    pub(crate) drop_callback: Option<Box<FnMut(&mut ClientRequest) -> result::Result<()> + Send>>,
}

impl Drop for ClientRequest {
    fn drop(&mut self) {
        if self.state() != SenderState::Done {
            warn!(
                "sender was not properly finished, state: {:?}, invoking custom callback",
                self.state()
            );
            if let Some(mut drop_callback) = mem::replace(&mut self.drop_callback, None) {
                if let Err(e) = drop_callback(self) {
                    warn!("custom callback resulted in error: {:?}", e);
                }
            }
        }
    }
}

fn _assert_types() {
    assert_send::<ClientRequest>();
}

impl ClientRequest {
    pub fn state(&self) -> SenderState {
        self.common.state()
    }

    pub fn set_drop_callback<F>(&mut self, f: F)
    where
        F: FnMut(&mut ClientRequest) -> result::Result<()> + Send + 'static,
    {
        self.drop_callback = Some(Box::new(f));
    }

    pub fn clear_drop_callback(&mut self) {
        mem::replace(&mut self.drop_callback, None);
    }

    /// Wait for stream to be ready to accept data.
    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        self.common.poll()
    }

    /// Synchronously wait till outgoing stream has non-zero space
    pub fn block_wait(&mut self) -> Result<(), StreamDead> {
        self.common.block_wait()
    }

    /// Enqueue data to outgoing stream
    ///
    /// This operation fails if stream is in incorrect state.
    ///
    /// The operation does not fail if stream or connection windows is not available,
    /// in that case message will be queued until peer increases window.
    pub fn send_data(&mut self, data: Bytes) -> Result<(), SendError> {
        self.common.send_data(data)
    }

    /// Send last `DATA` frame
    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> Result<(), SendError> {
        self.common.send_data_end_of_stream(data)
    }

    /// Send trailing headers
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

    pub fn reset(&mut self, error_code: ErrorCode) -> Result<(), SendError> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> Result<(), SendError> {
        self.common.close()
    }
}
