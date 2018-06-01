//! Single client connection

use std::result::Result as std_Result;
use std::sync::Arc;
use std::io;

use error;
use error::Error;
use result;

use exec::CpuPoolOption;

use solicit::StreamId;
use solicit::header::*;
use solicit::connection::EndStream;
use solicit::frame::settings::*;
use solicit::DEFAULT_SETTINGS;

use service::Service;

use futures::future::Future;
use futures::stream::Stream;
use futures::sync::oneshot;
use futures::sync::mpsc::unbounded;
use futures::sync::mpsc::UnboundedSender;

use tls_api::TlsConnector;

use tokio_core::reactor;
use tokio_timer::Timer;
use tokio_io::AsyncWrite;
use tokio_io::AsyncRead;
use tokio_tls_api;

use solicit_async::*;

use common::*;
use data_or_trailers::*;
use client_conf::*;
use client_tls::*;
use socket::*;

use rc_mut::*;
use req_resp::RequestOrResponse;
use headers_place::HeadersPlace;
use ErrorCode;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use result_or_eof::ResultOrEof;
use std::marker;


struct ClientTypes<I>(marker::PhantomData<I>);

impl<I> Types for ClientTypes<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Io = I;
    type HttpStreamData = ClientStream<I>;
    type HttpStreamSpecific = ClientStreamData;
    type ConnDataSpecific = ClientConnData;
    type ToWriteMessage = ClientToWriteMessage;
    type CommandMessage = ClientCommandMessage;

    fn out_request_or_response() -> RequestOrResponse {
        RequestOrResponse::Request
    }

    fn first_id() -> StreamId {
        1
    }
}



pub struct ClientStreamData {
}

impl HttpStreamDataSpecific for ClientStreamData {
}

type ClientStream<I> = HttpStreamCommon<ClientTypes<I>>;

impl<I> HttpStreamData for ClientStream<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Types = ClientTypes<I>;
}

pub struct ClientConnData {
    callbacks: Box<ClientConnectionCallbacks>,
}

impl ConnDataSpecific for ClientConnData {
}

type ClientInner<I> = ConnData<ClientTypes<I>>;

impl<I> ConnInner for ClientInner<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Types = ClientTypes<I>;

    fn process_headers(&mut self, _self_rc: RcMut<Self>, stream_id: StreamId, end_stream: EndStream, headers: Headers)
        -> result::Result<Option<HttpStreamRef<ClientTypes<I>>>>
    {
        let existing_stream = self.get_stream_for_headers_maybe_send_error(stream_id)?.is_some();
        if !existing_stream {
            return Ok(None);
        }

        let in_message_stage = self.streams.get_mut(stream_id).unwrap()
            .stream().in_message_stage;

        let headers_place = match in_message_stage {
            InMessageStage::Initial => HeadersPlace::Initial,
            InMessageStage::AfterInitialHeaders => HeadersPlace::Trailing,
            InMessageStage::AfterTrailingHeaders => {
                return Err(error::Error::InternalError(
                    format!("closed stream must be handled before")));
            },
        };

        if let Err(e) = headers.validate(RequestOrResponse::Response, headers_place) {
            warn!("invalid headers: {:?}: {:?}", e, headers);
            self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
            return Ok(None);
        }

        let status_1xx = match headers_place {
            HeadersPlace::Initial => {
                let status = headers.status();

                let status_1xx = status >= 100 && status <= 199;
                if status_1xx && end_stream == EndStream::Yes {
                    warn!("1xx headers and end stream: {}", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                    return Ok(None);
                }
                status_1xx
            }
            HeadersPlace::Trailing => {
                if end_stream == EndStream::No {
                    warn!("headers without end stream after data: {}", stream_id);
                    self.send_rst_stream(stream_id, ErrorCode::ProtocolError)?;
                    return Ok(None);
                }
                false
            }
        };

        let mut stream = self.streams.get_mut(stream_id).unwrap();
        if let Some(in_rem_content_length) = headers.content_length() {
            stream.stream().in_rem_content_length = Some(in_rem_content_length);
        }

        stream.stream().in_message_stage = match (headers_place, status_1xx) {
            (HeadersPlace::Initial, false) => InMessageStage::AfterInitialHeaders,
            (HeadersPlace::Initial, true) => InMessageStage::Initial,
            (HeadersPlace::Trailing, _) => InMessageStage::AfterTrailingHeaders,
        };

        // Ignore 1xx headers
        if !status_1xx {
            if let Some(ref mut response_handler) = stream.stream().peer_tx {
                // TODO: reset stream on error
                drop(response_handler.send(ResultOrEof::Item(DataOrHeadersWithFlag {
                    content: DataOrHeaders::Headers(headers),
                    last: end_stream == EndStream::Yes,
                })));
            } else {
                // TODO: reset stream
            }
        }

        Ok(Some(stream))
    }
}

pub struct ClientConnection {
    write_tx: UnboundedSender<ClientToWriteMessage>,
    command_tx: UnboundedSender<ClientCommandMessage>,
}

unsafe impl Sync for ClientConnection {}

pub struct StartRequestMessage {
    pub headers: Headers,
    pub body: HttpStreamAfterHeaders,
    pub resp_tx: oneshot::Sender<Response>,
}

enum ClientToWriteMessage {
    Start(StartRequestMessage),
    Common(CommonToWriteMessage),
}

impl From<CommonToWriteMessage> for ClientToWriteMessage {
    fn from(m: CommonToWriteMessage) -> Self {
        ClientToWriteMessage::Common(m)
    }
}

enum ClientCommandMessage {
    DumpState(oneshot::Sender<ConnectionStateSnapshot>),
    WaitForHandshake(oneshot::Sender<result::Result<()>>),
}


impl<I : AsyncWrite + Send + 'static> WriteLoopCustom for ClientWriteLoop<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Types = ClientTypes<I>;

    fn process_message(&mut self, message: ClientToWriteMessage) -> result::Result<()> {
        match message {
            ClientToWriteMessage::Start(start) => self.process_start(start),
            ClientToWriteMessage::Common(common) => self.process_common_message(common),
        }
    }
}

impl<I : AsyncWrite + Send + 'static> ClientWriteLoop<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    fn process_start(&mut self, start: StartRequestMessage)
        -> result::Result<()>
    {
        let StartRequestMessage { headers, body, resp_tx } = start;

        let stream_id = self.inner.with(move |inner: &mut ClientInner<I>| {

            let stream_id = inner.next_local_stream_id();

            let out_window = {
                let (mut http_stream, resp_stream, out_window) = inner.new_stream_data(
                    stream_id,
                    None,
                    InMessageStage::Initial,
                    ClientStreamData {});

                if let Err(_) = resp_tx.send(Response::from_stream(resp_stream)) {
                    warn!("caller died");
                }

                http_stream.stream().outgoing.push_back(DataOrHeaders::Headers(headers));

                out_window
            };

            inner.pump_stream_to_write_loop(stream_id, body.into_part_stream(), out_window);

            stream_id
        });

        // Also opens latch if necessary
        self.buffer_outg_stream(stream_id);
        Ok(())
    }
}

#[allow(dead_code)]
type ClientReadLoop<I> = ReadLoop<ClientTypes<I>>;
#[allow(dead_code)]
type ClientWriteLoop<I> = WriteLoop<ClientTypes<I>>;


pub trait ClientConnectionCallbacks : 'static {
    // called at most once
    fn goaway(&self, stream_id: StreamId, raw_error_code: u32);
}


impl ClientConnection {
    fn spawn_connected<I, C>(
        lh: reactor::Handle, connect: HttpFutureSend<I>,
        conf: ClientConf,
        callbacks: C)
            -> Self
        where
            I : AsyncWrite + AsyncRead + Send + 'static,
            C : ClientConnectionCallbacks,
    {
        let (to_write_tx, to_write_rx) = unbounded();
        let (command_tx, command_rx) = unbounded();

        let to_write_rx = Box::new(to_write_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write"))));
        let command_rx = Box::new(command_rx.map_err(|()| Error::IoError(io::Error::new(io::ErrorKind::Other, "to_write"))));

        let c = ClientConnection {
            write_tx: to_write_tx.clone(),
            command_tx: command_tx,
        };

        let settings_frame = SettingsFrame::from_settings(vec![ HttpSetting::EnablePush(false) ]);
        let mut settings = DEFAULT_SETTINGS;
        settings.apply_from_frame(&settings_frame);

        let handshake = connect.and_then(|conn| client_handshake(conn, settings_frame));

        let conn_data = ConnData::new(
            lh.clone(),
            CpuPoolOption::SingleThread,
            ClientConnData {
                callbacks: Box::new(callbacks),
            },
            conf.common,
            settings,
            to_write_tx.clone(),
            command_rx);

        let conn_data_error_holder = conn_data.conn_died_error_holder.clone();

        let future = handshake.and_then(move |conn| {
            debug!("handshake done");

            create_loops::<ClientTypes<_>>(conn_data, conn, to_write_rx)
        });

        let future = conn_data_error_holder.wrap_future(future);

        lh.spawn(future);

        c
    }

    pub fn spawn<H, C>(
        lh: reactor::Handle,
        addr: Box<ToClientStream>,
        tls: ClientTlsOption<C>,
        conf: ClientConf,
        callbacks: H) -> Self
        where H : ClientConnectionCallbacks, C : TlsConnector + Sync
    {
        match tls {
            ClientTlsOption::Plain =>
                ClientConnection::spawn_plain(lh.clone(), addr, conf, callbacks),
            ClientTlsOption::Tls(domain, connector) =>
                ClientConnection::spawn_tls(lh.clone(), &domain, connector, addr, conf, callbacks),
        }
    }

    pub fn spawn_plain<C>(
        lh: reactor::Handle,
        addr: Box<ToClientStream>,
        conf: ClientConf,
        callbacks: C)
            -> Self
        where C : ClientConnectionCallbacks
    {
        let no_delay = conf.no_delay.unwrap_or(true);
        let connect = addr.connect(&lh).map_err(Into::into);
        let map_callback = move |socket: Box<StreamItem>| {
            info!("connected to {}", addr);

            if socket.is_tcp() {
                socket.set_nodelay(no_delay).expect("failed to set TCP_NODELAY");
            }

            socket
        };

        let connect: Box<Future<Item=_, Error=_> + Send> = if let Some(timeout) = conf.connection_timeout {
            let timer = Timer::default();
            Box::new(timer.timeout(connect, timeout).map(map_callback))
        } else {
            Box::new(connect.map(map_callback))
        };

        ClientConnection::spawn_connected(lh, connect, conf, callbacks)
    }

    pub fn spawn_tls<H, C>(
        lh: reactor::Handle,
        domain: &str,
        connector: Arc<C>,
        addr: Box<ToClientStream>,
        conf: ClientConf,
        callbacks: H)
            -> Self
        where H : ClientConnectionCallbacks, C : TlsConnector + Sync
    {
        let domain = domain.to_owned();

        let connect = addr.connect(&lh)
            .map(move |c| { info!("connected to {}", addr); c })
            .map_err(|e| e.into());

        let tls_conn = connect.and_then(move |conn| {
            tokio_tls_api::connect_async(&*connector, &domain, conn).map_err(|e| {
                Error::IoError(io::Error::new(io::ErrorKind::Other, e))
            })
        });

        let tls_conn = tls_conn.map_err(Error::from);

        ClientConnection::spawn_connected(lh, Box::new(tls_conn), conf, callbacks)
    }

    pub fn start_request_with_resp_sender(
        &self,
        start: StartRequestMessage)
            -> Result<(), StartRequestMessage>
    {
        self.write_tx.unbounded_send(ClientToWriteMessage::Start(start))
            .map_err(|send_error| {
                match send_error.into_inner() {
                    ClientToWriteMessage::Start(start) => start,
                    _ => unreachable!(),
                }
            })
    }

    pub fn dump_state_with_resp_sender(&self, tx: oneshot::Sender<ConnectionStateSnapshot>) {
        // ignore error
        drop(self.command_tx.unbounded_send(ClientCommandMessage::DumpState(tx)));
    }

    /// For tests
    #[doc(hidden)]
    pub fn _dump_state(&self) -> HttpFutureSend<ConnectionStateSnapshot> {
        let (tx, rx) = oneshot::channel();

        self.dump_state_with_resp_sender(tx);

        let rx = rx.map_err(|_| Error::from(io::Error::new(io::ErrorKind::Other, "oneshot canceled")));

        Box::new(rx)
    }

    pub fn wait_for_connect_with_resp_sender(&self, tx: oneshot::Sender<result::Result<()>>)
        -> std_Result<(), oneshot::Sender<result::Result<()>>>
    {
        self.command_tx.unbounded_send(ClientCommandMessage::WaitForHandshake(tx))
            .map_err(|send_error| {
                match send_error.into_inner() {
                    ClientCommandMessage::WaitForHandshake(tx) => tx,
                    _ => unreachable!(),
                }
            })
    }
}

impl Service for ClientConnection {
    // TODO: copy-paste with Client::start_request
    fn start_request(
        &self,
        headers: Headers,
        body: HttpStreamAfterHeaders)
            -> Response
    {
        let (resp_tx, resp_rx) = oneshot::channel();

        let start = StartRequestMessage {
            headers: headers,
            body: body,
            resp_tx: resp_tx,
        };

        if let Err(_) = self.start_request_with_resp_sender(start) {
            return Response::err(error::Error::Other("client died"));
        }

        let resp_rx = resp_rx.map_err(|oneshot::Canceled| error::Error::Other("client likely died"));

        let resp_rx = resp_rx.map(|r| r.into_stream_flag());

        let resp_rx = resp_rx.flatten_stream();

        Response::from_stream(resp_rx)
    }
}

impl<I : AsyncRead + Send + 'static> ReadLoopCustom for ClientReadLoop<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Types = ClientTypes<I>;
}

impl<I> CommandLoopCustom for ClientInner<I>
    where I : AsyncWrite + AsyncRead + Send + 'static
{
    type Types = ClientTypes<I>;

    fn process_command_message(&mut self, message: ClientCommandMessage) -> result::Result<()> {
        match message {
            ClientCommandMessage::DumpState(sender) => self.process_dump_state(sender),
            ClientCommandMessage::WaitForHandshake(tx) => {
                // ignore error
                drop(tx.send(Ok(())));
                Ok(())
            },
        }
    }
}
