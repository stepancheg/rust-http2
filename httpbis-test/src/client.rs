use futures::future::Future;

use httpbis::for_test::ConnStateSnapshot;
use httpbis::for_test::HttpStreamStateSnapshot;
use httpbis::for_test::StreamId;
use httpbis::Client;

pub trait ClientExt {
    fn conn_state(&self) -> ConnStateSnapshot;
    fn stream_state(&self, stream_id: StreamId) -> HttpStreamStateSnapshot;
}

impl ClientExt for Client {
    fn conn_state(&self) -> ConnStateSnapshot {
        self.dump_state().wait().unwrap()
    }

    fn stream_state(&self, stream_id: StreamId) -> HttpStreamStateSnapshot {
        self.conn_state().streams[&stream_id].clone()
    }
}
