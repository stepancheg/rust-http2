use std::collections::VecDeque;

use stream_part::HttpStreamPart;
use stream_part::HttpStreamPartContent;

use error::ErrorCode;


fn data_size(content: &HttpStreamPartContent) -> usize {
    match *content {
        HttpStreamPartContent::Headers(_) => 0,
        HttpStreamPartContent::Data(ref d) => d.len(),
    }
}


// Outgoing frames queue
pub struct StreamQueue {
    // items, newest in back
    queue: VecDeque<HttpStreamPartContent>,
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

    pub fn push_back(&mut self, part: HttpStreamPartContent) {
        if let Some(_) = self.end {
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
