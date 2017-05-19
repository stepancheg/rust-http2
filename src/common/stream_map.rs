use solicit::StreamId;
use std::collections::HashMap;

use super::stream::HttpStreamCommon;
use super::types::Types;

pub struct StreamMap<T : Types> {
    pub map: HashMap<StreamId, HttpStreamCommon<T>>,
}

impl<T : Types> StreamMap<T> {
    pub fn new() -> StreamMap<T> {
        StreamMap {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: StreamId, stream: HttpStreamCommon<T>) {
        if let Some(..) = self.map.insert(id, stream) {
            // TODO: error instead of panic
            panic!("inserted stream that already existed");
        }
    }

    pub fn get_mut(&mut self, id: StreamId) -> Option<&mut HttpStreamCommon<T>> {
        self.map.get_mut(&id)
    }
}
