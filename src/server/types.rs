use crate::common::client_or_server::ClientOrServer;
use crate::common::types::Types;
use crate::req_resp::RequestOrResponse;
use crate::server::conn::ServerConnData;
use crate::server::conn::ServerStream;
use crate::server::conn::ServerStreamData;
use crate::server::conn::ServerToWriteMessage;
use crate::server::stream_handler::ServerStreamHandlerHolder;

#[derive(Clone)]
pub(crate) struct ServerTypes;

impl Types for ServerTypes {
    type HttpStreamData = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type ConnSpecific = ServerConnData;
    type StreamHandlerHolder = ServerStreamHandlerHolder;
    type ToWriteMessage = ServerToWriteMessage;

    const CLIENT_OR_SERVER: ClientOrServer = ClientOrServer::Server;
    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse = RequestOrResponse::Response;
    const CONN_NDC: &'static str = "server conn";
}
