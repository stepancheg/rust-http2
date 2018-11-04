use solicit::stream_id::StreamId;

/// Runtime check if something is a client or a server.
#[derive(Eq, PartialEq, Debug)]
pub enum ClientOrServer {
    Client,
    Server,
}

impl ClientOrServer {
    /// First stream id initiated by client or server
    pub fn first_stream_id(&self) -> StreamId {
        match self {
            ClientOrServer::Client => 1,
            ClientOrServer::Server => 2,
        }
    }

    pub fn who_initiated_stream(stream_id: StreamId) -> ClientOrServer {
        match stream_id % 2 == 0 {
            true => ClientOrServer::Server,
            false => ClientOrServer::Client,
        }
    }
}
