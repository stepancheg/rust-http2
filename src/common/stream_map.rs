use solicit::StreamId;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::collections::hash_map::OccupiedEntry;

use error::ErrorCode;

use solicit::session::StreamState;
use solicit::WindowSize;
use super::stream::HttpStreamCommon;
use super::stream::HttpStreamCommand;
use super::stream::HttpStreamStateSnapshot;
use super::types::Types;


pub struct StreamMap<T : Types> {
    pub map: HashMap<StreamId, HttpStreamCommon<T>>,
}

/// Reference to a stream within `StreamMap`
pub struct HttpStreamRef<'m, T : Types + 'm> {
    entry: OccupiedEntry<'m, StreamId, HttpStreamCommon<T>>,
}

impl<T : Types> StreamMap<T> {
    pub fn new() -> StreamMap<T> {
        StreamMap {
            map: HashMap::new(),
        }
    }

    /// Insert a stream into a map and return a reference to it
    pub fn insert(&mut self, id: StreamId, stream: HttpStreamCommon<T>) -> HttpStreamRef<T> {
        match self.map.entry(id) {
            Entry::Occupied(_) => panic!("stream to insert that already exists: {}", id),
            Entry::Vacant(v) => v.insert(stream),
        };

        // unfortunately HashMap doesn't have an API to convert vacant entry into occupied
        self.get_mut(id).unwrap()
    }

    pub fn get_mut(&mut self, id: StreamId) -> Option<HttpStreamRef<T>> {
        match self.map.entry(id) {
            Entry::Occupied(e) => Some(HttpStreamRef {
                entry: e,
            }),
            Entry::Vacant(_) => None,
        }
    }

    /// Remove locally initiated streams with id > given.
    pub fn remove_local_streams_with_id_gt(&mut self, id: StreamId)
        -> Vec<(StreamId, HttpStreamCommon<T>)>
    {
        let stream_ids: Vec<StreamId> = self.map.keys().cloned()
            .filter(|&s| s > id && T::is_init_locally(s))
            .collect();

        let mut r = Vec::new();
        for r_id in stream_ids {
            r.push((r_id, self.map.remove(&r_id).unwrap()))
        }
        r
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn stream_ids(&self) -> Vec<StreamId> {
        self.map.keys().cloned().collect()
    }

    pub fn snapshot(&self) -> HashMap<StreamId, HttpStreamStateSnapshot> {
        self.map.iter().map(|(&k, s)| (k, s.snapshot())).collect()
    }
}

impl <'m, T : Types + 'm> HttpStreamRef<'m, T> {
    pub fn id(&self) -> StreamId {
        *self.entry.key()
    }

    pub fn stream(&mut self) -> &mut HttpStreamCommon<T> {
        self.entry.get_mut()
    }

    pub fn _into_stream(self) -> &'m mut HttpStreamCommon<T> {
        self.entry.into_mut()
    }

    fn remove(self) {
        self.entry.remove();
    }

    pub fn remove_if_closed(mut self) {
        if self.stream().state == StreamState::Closed {
            debug!("removing stream {}, because it's closed", self.id());
            self.remove();
        }
    }

    pub fn pop_outg_all_maybe_remove(mut self, conn_out_window_size: &mut WindowSize)
        -> Vec<HttpStreamCommand>
    {
        let mut r = Vec::new();
        loop {
            if let Some(c) = self.stream().pop_outg(conn_out_window_size) {
                r.push(c);
            } else {
                self.remove_if_closed();
                return r;
            }
        }
    }

    // Reset stream and remove it
    pub fn rst_remove(mut self, error_code: ErrorCode) {
        self.stream().rst(error_code);
        self.stream().state = StreamState::Closed;
        self.remove_if_closed();
    }
}
