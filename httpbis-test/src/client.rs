use httpbis::for_test::ConnStateSnapshot;
use httpbis::for_test::HttpStreamStateSnapshot;
use httpbis::Client;
use httpbis::StreamId;
use tokio::runtime::Runtime;

pub trait ClientExtForTest {
    fn conn_state(&self) -> ConnStateSnapshot;
    fn stream_state(&self, stream_id: StreamId) -> HttpStreamStateSnapshot;
}

impl ClientExtForTest for Client {
    fn conn_state(&self) -> ConnStateSnapshot {
        Runtime::new().unwrap().block_on(self.dump_state()).unwrap()
    }

    fn stream_state(&self, stream_id: StreamId) -> HttpStreamStateSnapshot {
        self.conn_state().streams[&stream_id].clone()
    }
}
