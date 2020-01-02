use std::cmp;

use bytes::Bytes;

use crate::error;

use crate::solicit::end_stream::EndStream;
use crate::solicit::header::Headers;
use crate::solicit::session::StreamState;
use crate::solicit::WindowSize;

use super::types::Types;

use super::stream_queue::StreamQueue;
use super::window_size;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::ErrorCode;

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

#[must_use]
pub struct DroppedData {
    pub size: usize,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct HttpStreamStateSnapshot {
    pub state: StreamState,
    pub out_window_size: i32,
    pub in_window_size: i32,
    pub pump_out_window_size: isize,
    pub queued_out_data_size: usize,
    pub out_data_size: usize,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum InMessageStage {
    Initial,
    AfterInitialHeaders,
    AfterTrailingHeaders,
}

/// All HTTP/2 stream state.
/// Note the state must be kept in sync with other fields,
/// thus sometimes this object must be manipulated with `HttpStreamRef`.
pub(crate) struct HttpStreamCommon<T: Types> {
    pub specific: T::HttpStreamSpecific,
    pub state: StreamState,
    pub out_window_size: WindowSize,
    pub in_window_size: WindowSize,
    pub outgoing: StreamQueue,
    pub peer_tx: Option<T::StreamHandlerHolder>,
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
            peer_tx: None,
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
            pump_out_window_size: self.pump_out_window.get(),
            queued_out_data_size: self.outgoing.data_size(),
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
    }

    pub fn conn_died(mut self, error: error::Error) {
        if let Some(handler) = self.peer_tx.take() {
            drop(handler.error(error));
        }
    }

    /// Must be kept in sync with `pop_outg`.
    pub fn is_writable(&self) -> bool {
        match self.outgoing.front() {
            Some(front) => match front {
                DataOrHeaders::Headers(..) => true,
                DataOrHeaders::Data(data) => data.len() == 0 || self.out_window_size.size() != 0,
            },
            None => {
                if let Some(_error_code) = self.outgoing.end() {
                    if !self.state.is_closed_local() {
                        return true;
                    }
                };

                false
            }
        }
    }

    #[cfg(debug_assertions)]
    pub fn pop_outg(&mut self, conn_out_window_size: &mut WindowSize) -> Option<HttpStreamCommand> {
        let writable = self.is_writable();
        let window_size_before = conn_out_window_size.0;

        let command = self.pop_outg_impl(conn_out_window_size);
        if command.is_some() {
            assert!(writable);
        } else {
            assert!(!writable || window_size_before == 0);
        }
        command
    }

    #[cfg(not(debug_assertions))]
    pub fn pop_outg(&mut self, conn_out_window_size: &mut WindowSize) -> Option<HttpStreamCommand> {
        self.pop_outg_impl(conn_out_window_size)
    }

    fn pop_outg_impl(
        &mut self,
        conn_out_window_size: &mut WindowSize,
    ) -> Option<HttpStreamCommand> {
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

    pub fn data_recvd(&mut self, data: Bytes, last: bool) {
        if let Some(ref mut response_handler) = self.peer_tx {
            // TODO: reset stream if rx is dead
            drop(response_handler.data_frame(data, last));
        }
    }

    pub fn rst_recvd(&mut self, error_code: ErrorCode) -> DroppedData {
        if let Some(response_handler) = self.peer_tx.take() {
            drop(response_handler.rst(error_code));
        }
        DroppedData {
            size: self.outgoing.data_size(),
        }
    }

    pub fn goaway_recvd(&mut self, _raw_error_code: u32) {
        if let Some(response_handler) = self.peer_tx.take() {
            // it is OK to ignore error: handler may be already dead
            drop(response_handler.error(error::Error::GoawayReceived));
        }
    }
}

pub(crate) trait HttpStreamDataSpecific: Send + 'static {}

pub(crate) trait HttpStreamData {
    type Types: Types;
}
