use std::collections::VecDeque;
use std::cmp;

use futures::sync::mpsc::UnboundedSender;

use bytes::Bytes;

use error;

use solicit::session::StreamState;
use solicit::WindowSize;
use solicit::DEFAULT_SETTINGS;
use solicit::header::Headers;
use solicit::connection::EndStream;

use futures_misc::ResultOrEof;
use futures_misc::LatchController;

use stream_part::*;

use error::ErrorCode;

use super::types::Types;


pub enum HttpStreamCommand {
    Headers(Headers, EndStream),
    Data(Bytes, EndStream),
    Rst(ErrorCode),
}

impl HttpStreamCommand {
    pub fn from(part: HttpStreamPart) -> HttpStreamCommand {
        let end_stream = match part.last {
            true => EndStream::Yes,
            false => EndStream::No,
        };
        match part.content {
            HttpStreamPartContent::Data(data) => {
                HttpStreamCommand::Data(data, end_stream)
            },
            HttpStreamPartContent::Headers(headers) => {
                HttpStreamCommand::Headers(headers, end_stream)
            },
        }
    }
}


// Outgoing frames queue
pub struct StreamOutQueue {
    // items, newest in back
    queue: VecDeque<HttpStreamPartContent>,
    // nothing will be added to `outgoing`
    // None means data is maybe available
    // Some(NoError) means data is successfully generated
    outgoing_end: Option<ErrorCode>,
    data_size: usize,
}

fn data_size(content: &HttpStreamPartContent) -> usize {
    match *content {
        HttpStreamPartContent::Headers(_) => 0,
        HttpStreamPartContent::Data(ref d) => d.len(),
    }
}

impl StreamOutQueue {
    pub fn new() -> StreamOutQueue {
        StreamOutQueue {
            queue: VecDeque::new(),
            outgoing_end: None,
            data_size: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn push_back(&mut self, part: HttpStreamPartContent) {
        if let Some(_) = self.outgoing_end {
            return;
        }
        self.data_size += data_size(&part);
        self.queue.push_back(part);
    }

    pub fn push_back_part(&mut self, part: HttpStreamPart) {
        self.push_back(part.content);
        if part.last {
            self.close(ErrorCode::NoError);
        }
    }

    pub fn push_front(&mut self, part: HttpStreamPartContent) {
        self.data_size += data_size(&part);
        self.queue.push_front(part);
    }

    pub fn pop_front(&mut self) -> Option<HttpStreamPartContent> {
        if let Some(part) = self.queue.pop_front() {
            self.data_size -= data_size(&part);
            Some(part)
        } else {
            None
        }
    }

    pub fn front(&self) -> Option<&HttpStreamPartContent> {
        self.queue.front()
    }

    pub fn close(&mut self, error_code: ErrorCode) {
        if None == self.outgoing_end || Some(ErrorCode::NoError) == self.outgoing_end {
            self.outgoing_end = Some(error_code);
        }
    }

    pub fn end(&self) -> Option<ErrorCode> {
        if !self.is_empty() {
            None
        } else {
            self.outgoing_end
        }
    }
}


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct HttpStreamStateSnapshot {
    pub state: StreamState,
    pub out_window_size: i32,
    pub in_window_size: i32,
}


pub struct HttpStreamCommon<T : Types> {
    pub specific: T::HttpStreamSpecific,
    pub state: StreamState,
    pub out_window_size: WindowSize,
    pub in_window_size: WindowSize,
    pub outgoing: StreamOutQueue,
    // channel for data to be sent as request on server, and as response on client
    // TODO: client code should drive the stream instead of unbounded channel
    // it is necessary for proper flow control
    pub peer_tx: Option<UnboundedSender<ResultOrEof<HttpStreamPart, error::Error>>>,
    // task waiting for window increase
    pub ready_to_write: LatchController,
}

impl<T : Types> HttpStreamCommon<T> {
    pub fn new(
        out_window_size: u32,
        peer_tx: UnboundedSender<ResultOrEof<HttpStreamPart, error::Error>>,
        ready_to_write: LatchController,
        specific: T::HttpStreamSpecific)
            -> HttpStreamCommon<T>
    {
        HttpStreamCommon {
            specific: specific,
            state: StreamState::Open,
            in_window_size: WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32),
            out_window_size: WindowSize::new(out_window_size as i32),
            outgoing: StreamOutQueue::new(),
            peer_tx: Some(peer_tx),
            ready_to_write: ready_to_write,
        }
    }

    pub fn snapshot(&self) -> HttpStreamStateSnapshot {
        HttpStreamStateSnapshot {
            state: self.state,
            out_window_size: self.out_window_size.0,
            in_window_size: self.in_window_size.0,
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
            // it is OK to ignore error: handler may be already dead
            drop(response_handler.send(ResultOrEof::Eof));
        }
    }

    pub fn pop_outg(&mut self, conn_out_window_size: &mut WindowSize) -> Option<HttpStreamCommand> {
        if self.outgoing.is_empty() {
            return
                if let Some(error_code) = self.outgoing.end() {
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

        let pop_headers =
            if let &HttpStreamPartContent::Headers(..) = self.outgoing.front().unwrap() {
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
            return Some(HttpStreamCommand::from(HttpStreamPart {
                content: r,
                last: last,
            }))
        }

        if self.out_window_size.size() <= 0 || conn_out_window_size.size() <= 0 {
            return None
        }

        let mut data =
            if let Some(HttpStreamPartContent::Data(data)) = self.outgoing.pop_front() {
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
            self.outgoing.push_front(HttpStreamPartContent::Data(rem));
        };

        self.out_window_size.try_decrease(data.len() as i32).unwrap();
        conn_out_window_size.try_decrease(data.len() as i32).unwrap();

        let last = self.outgoing.end() == Some(ErrorCode::NoError);
        if last {
            self.close_local();
        }

        Some(HttpStreamCommand::from(HttpStreamPart {
            content: HttpStreamPartContent::Data(data),
            last: last,
        }))
    }

    pub fn _pop_outg_all(&mut self, conn_out_window_size: &mut WindowSize) -> Vec<HttpStreamCommand> {
        let mut r = Vec::new();
        while let Some(p) = self.pop_outg(conn_out_window_size) {
            r.push(p);
        }
        r
    }

    pub fn new_data_chunk(&mut self, data: &[u8], last: bool) {
        if let Some(ref mut response_handler) = self.peer_tx {
            // TODO: reset stream if rx is dead
            drop(response_handler.send(ResultOrEof::Item(HttpStreamPart {
                content: HttpStreamPartContent::Data(Bytes::from(data)),
                last: last,
            })));
        }
    }

    pub fn rst(&mut self, error_code: ErrorCode) {
        if let Some(ref mut response_handler) = self.peer_tx.take() {
            drop(response_handler.send(ResultOrEof::Error(error::Error::CodeError(error_code))));
        }
    }

    pub fn goaway_recvd(&mut self, _raw_error_code: u32) {
        if let Some(response_handler) = self.peer_tx.take() {
            // it is OK to ignore error: handler may be already dead
            drop(response_handler.send(ResultOrEof::Error(error::Error::Other("peer sent GOAWAY"))));
        }
    }
}


pub trait HttpStreamDataSpecific {
}

pub trait HttpStream {
    type Types : Types;
}


