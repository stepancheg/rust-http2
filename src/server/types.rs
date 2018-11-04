use server::conn::ServerStreamData;
use server::conn::ServerToWriteMessage;
use common::client_or_server::ClientOrServer;
use req_resp::RequestOrResponse;
use common::types::Types;
use server::conn::ServerStream;
use server::conn::ServerConnData;

pub struct ServerTypes;

impl Types for ServerTypes {
    type HttpStreamData = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type ConnSpecific = ServerConnData;
    type ToWriteMessage = ServerToWriteMessage;

    const CLIENT_OR_SERVER: ClientOrServer = ClientOrServer::Server;
    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse = RequestOrResponse::Response;
}
