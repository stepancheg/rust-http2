use std::collections::VecDeque;

use stream_part::DataOrHeadersWithFlag;
use data_or_headers::DataOrHeaders;

use error::ErrorCode;


pub fn data_size(content: &DataOrHeaders) -> usize {
    match *content {
        DataOrHeaders::Headers(_) => 0,
        DataOrHeaders::Data(ref d) => d.len(),
    }
}


// Outgoing frames queue
pub struct StreamQueue {
    // items, newest in back
    queue: VecDeque<DataOrHeaders>,
    // nothing will be added to `outgoing`
    // None means data is maybe available
    // Some(NoError) means data is successfully generated
    end: Option<ErrorCode>,
    data_size: usize,
}

impl StreamQueue {
    pub fn new() -> StreamQueue {
        StreamQueue {
            queue: VecDeque::new(),
            end: None,
            data_size: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn data_size(&self) -> usize {
        self.data_size
    }

    pub fn push_back(&mut self, part: DataOrHeaders) {
        if let Some(_) = self.end {
            return;
        }
        self.data_size += data_size(&part);
        self.queue.push_back(part);
    }

    pub fn push_back_part(&mut self, part: DataOrHeadersWithFlag) {
        self.push_back(part.content);
        if part.last {
            self.close(ErrorCode::NoError);
        }
    }

    pub fn push_front(&mut self, part: DataOrHeaders) {
        self.data_size += data_size(&part);
        self.queue.push_front(part);
    }

    pub fn pop_front(&mut self) -> Option<DataOrHeaders> {
        if let Some(part) = self.queue.pop_front() {
            self.data_size -= data_size(&part);
            Some(part)
        } else {
            None
        }
    }

    pub fn front(&self) -> Option<&DataOrHeaders> {
        self.queue.front()
    }

    pub fn close(&mut self, error_code: ErrorCode) {
        if None == self.end || Some(ErrorCode::NoError) == self.end {
            self.end = Some(error_code);
        }
    }

    pub fn end(&self) -> Option<ErrorCode> {
        if !self.is_empty() {
            None
        } else {
            self.end
        }
    }
}
