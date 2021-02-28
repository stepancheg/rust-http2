use crate::assert_types::assert_send;
use crate::client::types::ClientTypes;
use crate::common::sender::CommonSender;

use crate::common::sink_after_headers::SinkAfterHeaders;
use crate::DataOrTrailers;
use crate::ErrorCode;
use crate::Headers;
use crate::SenderState;
use bytes::Bytes;
use futures::stream::Stream;
use futures::task::Context;
use std::task::Poll;

/// Reference to outgoing stream on the client side.
// NOTE: keep in sync with ServerResponse
pub struct ClientRequest {
    pub(crate) common: CommonSender<ClientTypes>,
}

fn _assert_types() {
    assert_send::<ClientRequest>();
}

impl ClientRequest {
    pub fn state(&self) -> SenderState {
        self.common.state()
    }

    /// Wait for stream to be ready to accept data.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        self.common.poll(cx)
    }

    /// Enqueue data to outgoing stream
    ///
    /// This operation fails if stream is in incorrect state.
    ///
    /// The operation does not fail if stream or connection windows is not available,
    /// in that case message will be queued until peer increases window.
    pub fn send_data(&mut self, data: Bytes) -> crate::Result<()> {
        self.common.send_data(data)
    }

    /// Send last `DATA` frame
    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> crate::Result<()> {
        self.common.send_data_end_of_stream(data)
    }

    /// Send trailing headers
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

    pub fn reset(&mut self, error_code: ErrorCode) -> crate::Result<()> {
        self.common.reset(error_code)
    }

    pub fn close(&mut self) -> crate::Result<()> {
        self.common.close()
    }
}
