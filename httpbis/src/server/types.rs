use crate::common::client_or_server::ClientOrServer;
use crate::common::types::Types;
use crate::for_test::solicit::frame::SettingsFrame;
use crate::net::socket::SocketStream;
use crate::req_resp::RequestOrResponse;
use crate::server::conn::ServerConnData;
use crate::server::conn::ServerStream;
use crate::server::conn::ServerStreamData;
use crate::server::conn::ServerToWriteMessage;
use crate::server::stream_handler::ServerRequestStreamHandlerHolder;
use crate::solicit_async::server_handshake;
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Default)]
pub(crate) struct ServerTypes;

impl Types for ServerTypes {
    type HttpStreamData = ServerStream;
    type HttpStreamSpecific = ServerStreamData;
    type SideSpecific = ServerConnData;
    type StreamHandlerHolder = ServerRequestStreamHandlerHolder;
    type ToWriteMessage = ServerToWriteMessage;

    const CLIENT_OR_SERVER: ClientOrServer = ClientOrServer::Server;
    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse = RequestOrResponse::Response;
    const CONN_NDC: &'static str = "server conn";

    fn handshake<'a, I: SocketStream>(
        conn: &'a mut I,
        settings_frame: SettingsFrame,
    ) -> Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'a>> {
        Box::pin(server_handshake(conn, settings_frame))
    }
}
