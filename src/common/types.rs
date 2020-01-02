use crate::common::client_or_server::ClientOrServer;
use crate::common::conn::ConnSpecific;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::init_where::InitWhere;
use crate::common::stream::HttpStreamData;
use crate::common::stream::HttpStreamDataSpecific;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::req_resp::RequestOrResponse;
use crate::solicit::stream_id::StreamId;

/// Client or server type names for connection and stream
pub(crate) trait Types: Clone + 'static {
    type HttpStreamData: HttpStreamData;
    type HttpStreamSpecific: HttpStreamDataSpecific;
    type ConnSpecific: ConnSpecific;
    type StreamHandlerHolder: StreamHandlerInternal;
    // Message sent to write loop
    type ToWriteMessage: From<CommonToWriteMessage> + Send;

    /// Runtime check if this type is constructed for client or server
    const CLIENT_OR_SERVER: ClientOrServer;

    /// Outgoing messages are requests or responses
    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse;

    const CONN_NDC: &'static str;

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
