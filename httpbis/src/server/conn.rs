use std::panic;
use std::sync::Arc;

use crate::AnySocketAddr;

use crate::solicit::end_stream::EndStream;
use crate::solicit::header::*;

use futures::future;

use crate::common::types::Types;

use tokio::net::TcpStream;

use tls_api::TlsAcceptor;

use crate::solicit_async::*;

use crate::net::socket::SocketStream;

use crate::common::init_where::InitWhere;

use crate::common::conn::Conn;
use crate::common::conn::ConnStateSnapshot;
use crate::common::conn::SideSpecific;
use crate::common::conn_read::ConnReadSideCustom;
use crate::common::conn_write::CommonToWriteMessage;
use crate::common::conn_write::ConnWriteSideCustom;
use crate::common::sender::CommonSender;
use crate::common::stream::HttpStreamCommon;
use crate::common::stream::HttpStreamData;
use crate::common::stream::HttpStreamDataSpecific;
use crate::common::stream::InMessageStage;
use crate::common::stream_handler::StreamHandlerInternal;
use crate::common::stream_map::HttpStreamRef;
use crate::death::channel::DeathAwareSender;
use crate::death::channel::ErrorAwareDrop;
use crate::death::error_holder::ConnDiedType;
use crate::death::error_holder::SomethingDiedErrorHolder;
use crate::death::oneshot::death_aware_oneshot;
use crate::headers_place::HeadersPlace;
use crate::misc::any_to_string;
use crate::req_resp::RequestOrResponse;
use crate::server::handler::ServerHandler;
use crate::server::handler::ServerHandlerContext;
use crate::server::req::ServerRequest;
use crate::server::types::ServerTypes;
use crate::solicit::stream_id::StreamId;
use crate::ErrorCode;
use crate::ServerConf;
use crate::ServerResponse;
use crate::ServerTlsOption;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio::runtime::Handle;

pub struct ServerStreamData {}

impl HttpStreamDataSpecific for ServerStreamData {}

pub(crate) type ServerStream = HttpStreamCommon<ServerTypes>;

impl ServerStream {
    fn trailers_recvd(&mut self, headers: Headers) {
        if let Some(ref mut sender) = self.peer_tx {
            // TODO: reset on error
            sender.trailers(headers).ok();
        }
    }
}

impl HttpStreamData for ServerStream {
    type Types = ServerTypes;
}

pub(crate) struct ServerConnData {
    factory: Arc<dyn ServerHandler>,
}

impl SideSpecific for ServerConnData {}

#[allow(dead_code)] // https://github.com/rust-lang/rust/issues/42303
type ServerInner<I> = Conn<ServerTypes, I>;

impl<I> ServerInner<I>
where
    I: SocketStream,
{
    fn new_stream_from_client(
        &mut self,
        stream_id: StreamId,
        headers: Headers,
        end_stream: EndStream,
    ) -> crate::Result<HttpStreamRef<ServerTypes>> {
        if ServerTypes::init_where(stream_id) == InitWhere::Locally {
            return Err(crate::Error::InitiatedStreamWithServerIdFromClient(
                stream_id,
            ));
        }

        if stream_id <= self.last_peer_stream_id {
            return Err(crate::Error::StreamIdLeExistingStream(
                stream_id,
                self.last_peer_stream_id,
            ));
        }

        self.last_peer_stream_id = stream_id;

        debug!("new stream: {}", stream_id);

        let (_, out_window) = self.new_stream_data(
            stream_id,
            headers.content_length(),
            InMessageStage::AfterInitialHeaders,
            ServerStreamData {},
        );

        let in_window_size = self
            .streams
            .get_mut(stream_id)
            .unwrap()
            .stream()
            .in_window_size
            .size() as u32;

        let factory = self.specific.factory.clone();

        let sender = ServerResponse {
            common: CommonSender::new(stream_id, self.to_write_tx.clone(), out_window, false),
            drop_callback: None,
        };

        let context = ServerHandlerContext {
            loop_handle: self.loop_handle.clone(),
        };

        let mut stream_handler = None;
        let invoke_result = {
            let req = ServerRequest {
                headers,
                end_stream: end_stream == EndStream::Yes,
                stream_id,
                in_window_size,
                stream_handler: &mut stream_handler,
                to_write_tx: &self.to_write_tx,
            };

            panic::catch_unwind(panic::AssertUnwindSafe(|| {
                factory.start_request(context, req, sender)
            }))
        };

        let mut stream = self.streams.get_mut(stream_id).expect("get stream");

        match invoke_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warn!("handler returned error: {:?}", e);
                // Not closing stream because sender object
                // is now responsible for sending `RST_STREAM` on error.
            }
            Err(e) => {
                let e = any_to_string(e);
                warn!("handler panicked: {}", e);
            }
        }

        stream.stream().peer_tx = stream_handler;

        Ok(stream)
    }
}

pub(crate) enum ServerToWriteMessage {
    Common(CommonToWriteMessage),
}

impl ErrorAwareDrop for ServerToWriteMessage {
    type DiedType = ConnDiedType;

    fn drop_with_error(self, _error: crate::Error) {
        // TODO
    }
}

impl From<CommonToWriteMessage> for ServerToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ServerToWriteMessage::Common(m)
    }
}

impl<I> ConnWriteSideCustom for Conn<ServerTypes, I>
where
    I: SocketStream,
{
    type Types = ServerTypes;

    fn process_message(&mut self, message: ServerToWriteMessage) -> crate::Result<()> {
        match message {
            ServerToWriteMessage::Common(common) => self.process_common_message(common),
        }
    }
}

impl<I> ConnReadSideCustom for Conn<ServerTypes, I>
where
    I: SocketStream,
{
    type Types = ServerTypes;

    fn process_headers(
        &mut self,
        stream_id: StreamId,
        end_stream: EndStream,
        headers: Headers,
    ) -> crate::Result<Option<HttpStreamRef<ServerTypes>>> {
        let existing_stream = self
            .get_stream_for_headers_maybe_send_error(stream_id)?
            .is_some();

        let headers_place = match existing_stream {
            true => HeadersPlace::Trailing,
            false => HeadersPlace::Initial,
        };

        if let Err(e) = headers.validate(RequestOrResponse::Request, headers_place) {
            warn!("invalid headers: {:?} {:?}", e, headers);
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        if !existing_stream {
            return self
                .new_stream_from_client(stream_id, headers, end_stream)
                .map(Some);
        }

        if end_stream == EndStream::No {
            warn!("more headers without end stream flag");
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        stream.stream().trailers_recvd(headers);
        Ok(Some(stream))
    }
}

pub struct ServerConn {
    write_tx: DeathAwareSender<ServerToWriteMessage>,
    conn_died_error_holder: SomethingDiedErrorHolder<ConnDiedType>,
}

impl ServerConn {
    fn connected<F, I>(
        lh: &Handle,
        socket: impl Future<Output = crate::Result<I>> + Send,
        peer_addr: AnySocketAddr,
        conf: ServerConf,
        service: Arc<F>,
    ) -> (ServerConn, impl Future<Output = ()> + Send)
    where
        F: ServerHandler,
        I: SocketStream,
    {
        let (future, write_tx, conn_died_error_holder) = Conn::<ServerTypes, I>::new(
            lh.clone(),
            ServerConnData { factory: service },
            conf.common,
            future::ok((peer_addr, socket)),
        );

        (
            ServerConn {
                write_tx,
                conn_died_error_holder,
            },
            future,
        )
    }

    pub fn new<S, A>(
        lh: &Handle,
        socket: Pin<Box<dyn SocketStream>>,
        peer_addr: AnySocketAddr,
        tls: ServerTlsOption<A>,
        conf: ServerConf,
        service: Arc<S>,
    ) -> (ServerConn, impl Future<Output = ()> + Send)
    where
        S: ServerHandler,
        A: TlsAcceptor,
    {
        match tls {
            ServerTlsOption::Plain => {
                let socket = Box::pin(future::ok(socket));
                let (conn, f) = ServerConn::connected(lh, socket, peer_addr, conf, service);
                let f: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(f);
                (conn, f)
            }
            ServerTlsOption::Tls(acceptor) => {
                let socket: HttpFutureSend<_> = Box::pin(async move {
                    let tls_stream = acceptor.accept_with_socket(socket).await?;
                    debug!("TLS handshake done");
                    Ok(tls_stream)
                });
                let (conn, f) = ServerConn::connected(lh, socket, peer_addr, conf, service);
                let f: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(f);
                (conn, Box::pin(f))
            }
        }
    }

    pub fn new_plain_single_thread<S>(
        lh: &Handle,
        socket: TcpStream,
        peer_addr: SocketAddr,
        conf: ServerConf,
        service: Arc<S>,
    ) -> (ServerConn, impl Future<Output = ()> + Send)
    where
        S: ServerHandler,
    {
        let no_tls: ServerTlsOption<tls_api_stub::TlsAcceptor> = ServerTlsOption::Plain;
        ServerConn::new(
            lh,
            Box::pin(socket),
            AnySocketAddr::Inet(peer_addr),
            no_tls,
            conf,
            service,
        )
    }

    pub fn new_plain_single_thread_fn<F>(
        lh: &Handle,
        socket: TcpStream,
        peer_addr: SocketAddr,
        conf: ServerConf,
        f: F,
    ) -> (ServerConn, impl Future<Output = ()> + Send)
    where
        F: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> crate::Result<()>
            + Send
            + Sync
            + 'static,
    {
        struct HttpServiceFn<F>(F);

        impl<F> ServerHandler for HttpServiceFn<F>
        where
            F: Fn(ServerHandlerContext, ServerRequest, ServerResponse) -> crate::Result<()>
                + Send
                + Sync
                + 'static,
        {
            fn start_request(
                &self,
                context: ServerHandlerContext,
                req: ServerRequest,
                resp: ServerResponse,
            ) -> crate::Result<()> {
                (self.0)(context, req, resp)
            }
        }

        ServerConn::new_plain_single_thread(lh, socket, peer_addr, conf, Arc::new(HttpServiceFn(f)))
    }

    /// For tests
    pub fn dump_state(&self) -> HttpFutureSend<ConnStateSnapshot> {
        let (tx, rx) = death_aware_oneshot(self.conn_died_error_holder.clone());

        if let Err(_) = self.write_tx.unbounded_send(ServerToWriteMessage::Common(
            CommonToWriteMessage::DumpState(tx),
        )) {
            return Box::pin(future::err(crate::Error::FailedToSendReqToDumpState));
        }

        Box::pin(rx)
    }
}
