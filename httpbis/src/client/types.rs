use crate::client::conn::ClientConnData;
use crate::client::conn::ClientStream;
use crate::client::conn::ClientStreamData;
use crate::client::conn::ClientToWriteMessage;
use crate::client::stream_handler::ClientResponseStreamHandlerHolder;
use crate::common::client_or_server::ClientOrServer;
use crate::common::types::Types;
use crate::net::socket::SocketStream;
use crate::req_resp::RequestOrResponse;
use crate::solicit::frame::SettingsFrame;
use crate::solicit_async::client_handshake;
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Default)]
pub struct ClientTypes;

impl Types for ClientTypes {
    type HttpStreamData = ClientStream;
    type HttpStreamSpecific = ClientStreamData;
    type SideSpecific = ClientConnData;
    type StreamHandlerHolder = ClientResponseStreamHandlerHolder;
    type ToWriteMessage = ClientToWriteMessage;

    const CLIENT_OR_SERVER: ClientOrServer = ClientOrServer::Client;

    const OUT_REQUEST_OR_RESPONSE: RequestOrResponse = RequestOrResponse::Request;
    const CONN_NDC: &'static str = "client conn";

    fn handshake<'a, I: SocketStream>(
        conn: &'a mut I,
        settings_frame: SettingsFrame,
    ) -> Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'a>> {
        Box::pin(client_handshake(conn, settings_frame))
    }
}
