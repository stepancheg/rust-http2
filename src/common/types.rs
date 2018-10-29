use solicit::StreamId;

use super::*;
use common::client_or_server::ClientOrServer;
use common::init_where::InitWhere;
use req_resp::RequestOrResponse;
use tokio_io::AsyncRead;
use tokio_io::AsyncWrite;

/// Client or server type names for connection and stream
// TODO: make Types without Io
pub trait Types: 'static {
    type Io: AsyncWrite + AsyncRead + Send + 'static;
    type HttpStreamData: HttpStreamData;
    type HttpStreamSpecific: HttpStreamDataSpecific;
    type ConnSpecific: ConnSpecific;
    // Message sent to write loop
    type ToWriteMessage: From<CommonToWriteMessage> + Send;

    /// Runtime check if this type is constructed for client or server
    const CLIENT_OR_SERVER: ClientOrServer;

    /// Outgoing messages are requests or responses
    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse;

    /// Is stream initiated locally or by peer?
    /// e. g. `is_init_locally(3)` returns `true` for client and `false` for server.
    fn init_where(stream_id: StreamId) -> InitWhere {
        let initiated_by_client_or_server = ClientOrServer::who_initiated_stream(stream_id);
        match initiated_by_client_or_server == Self::CLIENT_OR_SERVER {
            true => InitWhere::Locally,
            false => InitWhere::Peer,
        }
    }
}
