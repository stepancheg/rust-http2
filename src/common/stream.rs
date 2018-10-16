use std::cmp;

use bytes::Bytes;

use error;

use solicit::connection::EndStream;
use solicit::header::Headers;
use solicit::session::StreamState;
use solicit::WindowSize;

use result_or_eof::ResultOrEof;

use error::ErrorCode;

use super::types::Types;

use super::stream_queue::StreamQueue;
use super::stream_queue_sync::StreamQueueSyncSender;
use super::window_size;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;

pub enum HttpStreamCommand {
    Headers(Headers, EndStream),
    Data(Bytes, EndStream),
    Rst(ErrorCode),
}

impl HttpStreamCommand {
    pub fn from(part: DataOrHeadersWithFlag) -> HttpStreamCommand {
        let end_stream = match part.last {
            true => EndStream::Yes,
            false => EndStream::No,
        };
        match part.content {
            DataOrHeaders::Data(data) => HttpStreamCommand::Data(data, end_stream),
            DataOrHeaders::Headers(headers) => HttpStreamCommand::Headers(headers, end_stream),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct HttpStreamStateSnapshot {
    pub state: StreamState,
    pub out_window_size: i32,
    pub in_window_size: i32,
    pub out_data_size: usize,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum InMessageStage {
    Initial,
    AfterInitialHeaders,
    AfterTrailingHeaders,
}

pub struct HttpStreamCommon<T: Types> {
    pub specific: T::HttpStreamSpecific,
    pub state: StreamState,
    pub out_window_size: WindowSize,
    pub in_window_size: WindowSize,
    pub outgoing: StreamQueue,
    pub peer_tx: Option<StreamQueueSyncSender>,
    // task waiting for window increase
    pub pump_out_window: window_size::StreamOutWindowSender,
    // Incoming remaining content-length
    pub in_rem_content_length: Option<u64>,
    pub in_message_stage: InMessageStage,
}

impl<T: Types> HttpStreamCommon<T> {
    pub fn new(
        in_window_size: u32,
        out_window_size: u32,
        incoming: StreamQueueSyncSender,
        pump_out_window: window_size::StreamOutWindowSender,
        in_rem_content_length: Option<u64>,
        in_message_stage: InMessageStage,
        specific: T::HttpStreamSpecific,
    ) -> HttpStreamCommon<T> {
        HttpStreamCommon {
            specific,
            state: StreamState::Open,
            in_window_size: WindowSize::new(in_window_size as i32),
            out_window_size: WindowSize::new(out_window_size as i32),
            outgoing: StreamQueue::new(),
            peer_tx: Some(incoming),
            pump_out_window,
            in_rem_content_length,
            in_message_stage,
        }
    }

    pub fn snapshot(&self) -> HttpStreamStateSnapshot {
        HttpStreamStateSnapshot {
            state: self.state,
            out_window_size: self.out_window_size.0,
            in_window_size: self.in_window_size.0,
            out_data_size: self.outgoing.data_size(),
        }
    }

    pub fn close_local(&mut self) {
        trace!("close local");
        self.state = match self.state {
            StreamState::Closed | StreamState::HalfClosedRemote => StreamState::Closed,
            _ => StreamState::HalfClosedLocal,
        };
    }

    pub fn close_remote(&mut self) {
        trace!("close remote");
        self.state = match self.state {
            StreamState::Closed | StreamState::HalfClosedLocal => StreamState::Closed,
            _ => StreamState::HalfClosedRemote,
        };

        if let Some(response_handler) = self.peer_tx.take() {
            // TODO: reset on error
            drop(response_handler.send(ResultOrEof::Eof));
        }
    }

    pub fn pop_outg(&mut self, conn_out_window_size: &mut WindowSize) -> Option<HttpStreamCommand> {
        if self.outgoing.is_empty() {
            return if let Some(error_code) = self.outgoing.end() {
                if self.state.is_closed_local() {
                    None
                } else {
                    self.close_local();
                    Some(match error_code {
                        ErrorCode::NoError => HttpStreamCommand::Data(Bytes::new(), EndStream::Yes),
                        error_code => HttpStreamCommand::Rst(error_code),
                    })
                }
            } else {
                None
            };
        }

        let pop_headers = if let &DataOrHeaders::Headers(..) = self.outgoing.front().unwrap() {
            true
        } else {
            false
        };
        if pop_headers {
            let r = self.outgoing.pop_front().unwrap();
            let last = self.outgoing.end() == Some(ErrorCode::NoError);
            if last {
                self.close_local();
            }
            return Some(HttpStreamCommand::from(DataOrHeadersWithFlag {
                content: r,
                last: last,
            }));
        }

        if self.out_window_size.size() <= 0 || conn_out_window_size.size() <= 0 {
            return None;
        }

        let mut data = if let Some(DataOrHeaders::Data(data)) = self.outgoing.pop_front() {
            data
        } else {
            unreachable!()
        };

        // Min of connection and stream window size
        let max_window = cmp::min(self.out_window_size.size(), conn_out_window_size.size());

        if data.len() as usize > max_window as usize {
            trace!("truncating data of len {} to {}", data.len(), max_window);
            let size = max_window as usize;
            let rem = data.split_off(size);
            self.outgoing.push_front(DataOrHeaders::Data(rem));
        };

        self.out_window_size
            .try_decrease_to_positive(data.len() as i32)
            .unwrap();
        conn_out_window_size
            .try_decrease_to_positive(data.len() as i32)
            .unwrap();

        let last = self.outgoing.end() == Some(ErrorCode::NoError);
        if last {
            self.close_local();
        }

        Some(HttpStreamCommand::from(DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            last: last,
        }))
    }

    pub fn _pop_outg_all(
        &mut self,
        conn_out_window_size: &mut WindowSize,
    ) -> Vec<HttpStreamCommand> {
        let mut r = Vec::new();
        while let Some(p) = self.pop_outg(conn_out_window_size) {
            r.push(p);
        }
        r
    }

    pub fn new_data_chunk(&mut self, data: Bytes, last: bool) {
        if let Some(ref mut response_handler) = self.peer_tx {
            // TODO: reset stream if rx is dead
            drop(
                response_handler.send(ResultOrEof::Item(DataOrHeadersWithFlag {
                    content: DataOrHeaders::Data(data),
                    last: last,
                })),
            );
        }
    }

    pub fn rst_recvd(&mut self, error_code: ErrorCode) {
        if let Some(ref mut response_handler) = self.peer_tx.take() {
            drop(response_handler.send(ResultOrEof::Error(error::Error::CodeError(error_code))));
        }
    }

    pub fn goaway_recvd(&mut self, _raw_error_code: u32) {
        if let Some(response_handler) = self.peer_tx.take() {
            // it is OK to ignore error: handler may be already dead
            drop(
                response_handler.send(ResultOrEof::Error(error::Error::Other("peer sent GOAWAY"))),
            );
        }
    }
}

pub trait HttpStreamDataSpecific {}

pub trait HttpStreamData {
    type Types: Types;
}
