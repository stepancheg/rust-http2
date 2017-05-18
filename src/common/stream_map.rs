use solicit::StreamId;
use std::collections::HashMap;

use super::stream::HttpStream;

#[derive(Debug)]
pub struct StreamMap<S>
    where S : HttpStream
{
    pub map: HashMap<StreamId, S>,
}

impl<S : HttpStream> StreamMap<S> {
    pub fn new() -> StreamMap<S> {
        StreamMap {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: StreamId, stream: S) {
        if let Some(..) = self.map.insert(id, stream) {
            // TODO: error instead of panic
            panic!("inserted stream that already existed");
        }
    }

    pub fn get_mut(&mut self, id: StreamId) -> Option<&mut S> {
        self.map.get_mut(&id)
    }
}
