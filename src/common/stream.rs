use std::collections::VecDeque;
use std::cmp;

use bytes::Bytes;

use solicit::StreamId;
use solicit::session::StreamState;
use solicit::WindowSize;
use solicit::DEFAULT_SETTINGS;
use solicit::header::Headers;
use solicit::connection::EndStream;

use stream_part::*;

use error::ErrorCode;


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


pub struct HttpStreamCommon {
    pub state: StreamState,
    pub out_window_size: WindowSize,
    pub in_window_size: WindowSize,
    pub outgoing: VecDeque<HttpStreamPartContent>,
    // Means nothing will be added to `outgoing`
    pub outgoing_end: Option<ErrorCode>,
}

impl HttpStreamCommon {
    pub fn new(out_window_size: u32) -> HttpStreamCommon {
        HttpStreamCommon {
            state: StreamState::Open,
            in_window_size: WindowSize::new(DEFAULT_SETTINGS.initial_window_size as i32),
            out_window_size: WindowSize::new(out_window_size as i32),
            outgoing: VecDeque::new(),
            outgoing_end: None,
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
        self.state = match self.state {
            StreamState::Closed | StreamState::HalfClosedLocal => StreamState::Closed,
            _ => StreamState::HalfClosedRemote,
        };
    }

    pub fn pop_outg(&mut self, conn_out_window_size: &mut WindowSize) -> Option<HttpStreamCommand> {
        if self.outgoing.is_empty() {
            return
                if let Some(error_code) = self.outgoing_end {
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
            let last = self.outgoing_end == Some(ErrorCode::NoError) && self.outgoing.is_empty();
            if last {
                self.close_local();
            }
            return Some(HttpStreamCommand::from(HttpStreamPart {
                content: r,
                last: last,
            }))
        }

        if self.out_window_size.size() <= 0 {
            return None
        }

        let mut data =
            if let Some(HttpStreamPartContent::Data(data)) = self.outgoing.pop_front() {
                data
            } else {
                unreachable!()
            };

        // Max of connection and stream window size
        let max_window = cmp::min(self.out_window_size.size(), conn_out_window_size.size());

        if data.len() as usize > max_window as usize {
            trace!("truncating data of len {} to {}", data.len(), max_window);
            let size = max_window as usize;
            let rem = data.split_off(size);
            self.outgoing.push_front(HttpStreamPartContent::Data(rem));
        };

        self.out_window_size.try_decrease(data.len() as i32).unwrap();
        conn_out_window_size.try_decrease(data.len() as i32).unwrap();

        let last = self.outgoing_end == Some(ErrorCode::NoError) && self.outgoing.is_empty();
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
}


pub trait HttpStream {
    // First stream id used by either client or server
    fn first_id() -> StreamId;

    fn common(&self) -> &HttpStreamCommon;
    fn common_mut(&mut self) -> &mut HttpStreamCommon;
    fn new_data_chunk(&mut self, data: &[u8], last: bool);
    fn rst(&mut self, error_code: ErrorCode);
    fn closed_remote(&mut self);
}


