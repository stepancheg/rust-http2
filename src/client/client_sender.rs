use bytes::Bytes;
use client::client_conn::ClientToWriteMessage;
use common::conn_write::CommonToWriteMessage;
use common::window_size::StreamDead;
use common::window_size::StreamOutWindowReceiver;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use error;
use futures::future;
use futures::sync::mpsc;
use futures::sync::mpsc::UnboundedSender;
use futures::Future;
use futures::Poll;
use result;
use solicit::StreamId;
use ErrorCode;
use Headers;

pub struct ClientSender {
    pub(crate) stream_id: StreamId,
    pub(crate) write_tx: UnboundedSender<ClientToWriteMessage>,
    pub(crate) out_window: StreamOutWindowReceiver,
}

impl ClientSender {
    pub fn poll(&mut self) -> Poll<(), StreamDead> {
        self.out_window.poll()
    }

    pub fn wait(&mut self) -> Result<(), StreamDead> {
        future::poll_fn(|| self.poll()).wait()
    }

    fn send_common(&mut self, message: CommonToWriteMessage) -> result::Result<()> {
        // TODO: why client died?
        self.write_tx
            .unbounded_send(ClientToWriteMessage::Common(message))
            .map_err(|_: mpsc::SendError<_>| error::Error::ClientDied(None))
    }

    pub fn send_data(&mut self, data: Bytes) -> result::Result<()> {
        let stream_id = self.stream_id;
        self.out_window.decrease(data.len());
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                last: false,
            },
        ))
    }

    pub fn send_data_end_of_stream(&mut self, data: Bytes) -> result::Result<()> {
        let stream_id = self.stream_id;
        self.out_window.decrease(data.len());
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                last: true,
            },
        ))
    }

    pub fn send_trailers(&mut self, trailers: Headers) -> result::Result<()> {
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnqueue(
            stream_id,
            DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(trailers),
                last: true,
            },
        ))
    }

    pub fn reset(&mut self, error_code: ErrorCode) -> result::Result<()> {
        // TODO: do nothing if stream is explicitly closed
        let stream_id = self.stream_id;
        self.send_common(CommonToWriteMessage::StreamEnd(stream_id, error_code))
    }

    pub fn close(&mut self) -> result::Result<()> {
        self.reset(ErrorCode::NoError)
    }
}

impl Drop for ClientSender {
    fn drop(&mut self) {
        // Not sure correct code
        drop(self.reset(ErrorCode::Cancel))
    }
}
