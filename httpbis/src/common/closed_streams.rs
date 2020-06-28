use crate::solicit::stream_id::StreamId;
use std::collections::HashSet;
use std::collections::VecDeque;

/// HTTP/2 requires different behavior on closed streams depending on
/// who closed the stream: we or peer.
/// This struct tracks several recently closed streams.
#[derive(Default)]
pub struct ClosedStreams {
    set: HashSet<StreamId>,
    lru: VecDeque<StreamId>,
}

const MAX_SIZE: usize = 100;

impl ClosedStreams {
    pub fn new() -> ClosedStreams {
        Default::default()
    }

    pub fn contains(&self, stream_id: StreamId) -> bool {
        self.set.get(&stream_id).is_some()
    }

    pub fn add(&mut self, stream_id: StreamId) {
        if self.set.insert(stream_id) {
            if self.lru.len() == MAX_SIZE {
                let remove = self.lru.pop_front().unwrap();
                assert!(self.set.remove(&remove));
            }

            self.lru.push_back(stream_id);
        }
    }

    #[cfg(test)]
    pub fn self_check(&self) {
        assert_eq!(self.set.len(), self.lru.len());
        for stream_id in &self.lru {
            assert!(self.set.get(stream_id).is_some());
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let mut closed_streams = ClosedStreams::new();

        for i in 1..=MAX_SIZE {
            closed_streams.add(i as StreamId);
            closed_streams.self_check();
            assert!(closed_streams.contains(i as StreamId));
        }

        for i in 1..=MAX_SIZE {
            assert!(closed_streams.contains(i as StreamId));
        }

        for i in 1..=MAX_SIZE {
            closed_streams.add((i + 10000) as StreamId);
            assert!(closed_streams.contains((i + 10000) as StreamId));
            assert!(!closed_streams.contains(i as StreamId));
            closed_streams.self_check();
        }
    }
}
