use std::collections::hash_map::Entry;
use std::collections::hash_map::OccupiedEntry;
use std::collections::HashMap;

use super::stream::HttpStreamCommand;
use super::stream::HttpStreamCommon;
use super::stream::HttpStreamStateSnapshot;
use super::types::Types;
use common::hash_set_shallow_clone::HashSetShallowClone;
use common::hash_set_shallow_clone::HashSetShallowCloneItems;
use common::init_where::InitWhere;
use common::stream::DroppedData;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use solicit::session::StreamState;
use solicit::stream_id::StreamId;
use solicit::WindowSize;
use ErrorCode;

#[derive(Default)]
pub(crate) struct StreamMap<T: Types> {
    pub map: HashMap<StreamId, HttpStreamCommon<T>>,
    // This field must be kept in sync with stream state.
    writable_streams: HashSetShallowClone<StreamId>,
}

/// Reference to a stream within `StreamMap`
pub(crate) struct HttpStreamRef<'m, T: Types + 'm> {
    entry: OccupiedEntry<'m, StreamId, HttpStreamCommon<T>>,
    writable_streams: &'m mut HashSetShallowClone<StreamId>,
}

impl<T: Types> StreamMap<T> {
    pub fn new() -> StreamMap<T> {
        StreamMap {
            map: HashMap::new(),
            writable_streams: HashSetShallowClone::new(),
        }
    }

    /// Insert a stream into a map and return a reference to it
    pub fn insert(&mut self, id: StreamId, stream: HttpStreamCommon<T>) -> HttpStreamRef<T> {
        match self.map.entry(id) {
            Entry::Occupied(_) => panic!("stream to insert that already exists: {}", id),
            Entry::Vacant(v) => v.insert(stream),
        };

        // unfortunately HashMap doesn't have an API to convert vacant entry into occupied
        let mut stream = self.get_mut(id).unwrap();
        stream.sync_writable();
        stream
    }

    pub fn get_mut(&mut self, id: StreamId) -> Option<HttpStreamRef<T>> {
        match self.map.entry(id) {
            Entry::Occupied(e) => Some(HttpStreamRef {
                entry: e,
                writable_streams: &mut self.writable_streams,
            }),
            Entry::Vacant(_) => None,
        }
    }

    pub fn remove_stream(&mut self, id: StreamId) {
        if let Some(r) = self.get_mut(id) {
            r.remove();
        }
    }

    pub fn get_stream_state(&self, id: StreamId) -> Option<StreamState> {
        self.map.get(&id).map(|s| s.state)
    }

    /// Remove locally initiated streams with id > given.
    pub fn remove_local_streams_with_id_gt(
        &mut self,
        id: StreamId,
    ) -> Vec<(StreamId, HttpStreamCommon<T>)> {
        let stream_ids: Vec<StreamId> = self
            .map
            .keys()
            .cloned()
            .filter(|&s| s > id && T::init_where(s) == InitWhere::Locally)
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

    pub fn _stream_ids(&self) -> Vec<StreamId> {
        self.map.keys().cloned().collect()
    }

    pub fn writable_stream_ids(&mut self) -> HashSetShallowCloneItems<StreamId> {
        self.writable_streams.items()
    }

    pub fn snapshot(&self) -> HashMap<StreamId, HttpStreamStateSnapshot> {
        self.map.iter().map(|(&k, s)| (k, s.snapshot())).collect()
    }
}

impl<'m, T: Types + 'm> HttpStreamRef<'m, T> {
    pub fn stream(&mut self) -> &mut HttpStreamCommon<T> {
        self.entry.get_mut()
    }

    pub fn stream_ref(&self) -> &HttpStreamCommon<T> {
        self.entry.get()
    }

    pub fn id(&self) -> StreamId {
        *self.entry.key()
    }

    pub fn _into_stream(self) -> &'m mut HttpStreamCommon<T> {
        self.entry.into_mut()
    }

    fn remove(self) {
        let stream_id = self.id();
        debug!("removing stream {}", stream_id);
        self.writable_streams.remove(&stream_id);
        self.entry.remove();
    }

    fn is_writable(&self) -> bool {
        self.writable_streams.get(&self.id()).is_some()
    }

    fn check_state(&self) {
        debug_assert_eq!(
            self.stream_ref().is_writable(),
            self.is_writable(),
            "for stream {}",
            self.id()
        );
    }

    fn mark_writable(&mut self, writable: bool) {
        let stream_id = self.id();
        if writable {
            self.writable_streams.insert(stream_id);
        } else {
            self.writable_streams.remove(&stream_id);
        }
    }

    fn sync_writable(&mut self) {
        let writable = self.stream().is_writable();
        self.mark_writable(writable);
    }

    pub fn remove_if_closed(mut self) -> Option<Self> {
        if self.stream().state == StreamState::Closed {
            self.remove();
            None
        } else {
            Some(self)
        }
    }

    pub fn pop_outg_maybe_remove(
        mut self,
        conn_out_window_size: &mut WindowSize,
    ) -> (Option<HttpStreamCommand>, Option<Self>) {
        self.check_state();

        let r = self.stream().pop_outg(conn_out_window_size);

        self.sync_writable();

        let stream = self.remove_if_closed();
        (r, stream)
    }

    // Reset stream and remove it
    pub fn rst_received_remove(mut self, error_code: ErrorCode) -> DroppedData {
        let r = self.stream().rst_recvd(error_code);
        self.remove();
        r
    }

    pub fn try_increase_window_size(&mut self, increment: u32) -> Result<(), ()> {
        let old_window_size = self.stream().out_window_size.0;

        self.stream().out_window_size.try_increase(increment)?;

        let new_window_size = self.stream().out_window_size.0;

        debug!(
            "stream {} out window size change: {} -> {}",
            self.id(),
            old_window_size,
            new_window_size
        );

        self.sync_writable();

        Ok(())
    }

    pub fn push_back(&mut self, frame: DataOrHeaders) {
        self.stream().outgoing.push_back(frame);
        self.sync_writable();
    }

    pub fn push_back_part(&mut self, part: DataOrHeadersWithFlag) {
        self.stream().outgoing.push_back_part(part);
        self.sync_writable();
    }

    pub fn close_outgoing(&mut self, error_core: ErrorCode) {
        self.stream().outgoing.close(error_core);
        self.sync_writable();
    }

    pub fn close_remote(mut self) {
        self.stream().close_remote();
        self.remove_if_closed();
    }
}
