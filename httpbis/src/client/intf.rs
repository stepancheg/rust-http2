use crate::client::resp::ClientResponse;
use crate::client::resp_future::ClientResponseFuture3;
use crate::client::resp_future::ClientResponseFutureImpl;
use crate::common::sink_after_headers::SinkAfterHeadersBox;
use crate::solicit_async::TryFutureBox;
use crate::ClientHandler;
use crate::Header;
use crate::Headers;
use crate::HttpScheme;
use crate::PseudoHeaderName;
use bytes::Bytes;
use futures::channel::oneshot;
use futures::future;
use futures::FutureExt;
use futures::TryFutureExt;

#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ClientInternals {
    pub(crate) http_scheme: HttpScheme,
}

/// Client operations.
///
/// The main implementation of this interface is [`Client`](crate::Client).
pub trait ClientIntf {
    /// Start HTTP/2 request.
    fn start_request_low_level(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
        stream_handler: Box<dyn ClientHandler>,
    );

    #[doc(hidden)]
    fn internals(&self) -> &ClientInternals;

    /// Start a request, return a future resolved when request started.
    fn start_request(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
        end_stream: bool,
    ) -> TryFutureBox<(SinkAfterHeadersBox, ClientResponseFutureImpl)> {
        let (tx, rx) = oneshot::channel();

        struct Impl {
            tx: oneshot::Sender<crate::Result<(SinkAfterHeadersBox, ClientResponseFutureImpl)>>,
        }

        impl ClientHandler for Impl {
            fn request_created(
                self: Box<Self>,
                req: SinkAfterHeadersBox,
                resp: ClientResponse,
            ) -> crate::Result<()> {
                if let Err(_) = self.tx.send(Ok((req, resp.into_stream()))) {
                    return Err(crate::Error::CallerDied);
                }

                Ok(())
            }

            fn error(self: Box<Self>, error: crate::Error) {
                let _ = self.tx.send(Err(error));
            }
        }

        self.start_request_low_level(headers, body, trailers, end_stream, Box::new(Impl { tx }));

        let resp_rx = rx.then(move |r| match r {
            Ok(Ok(r)) => future::ok(r),
            Ok(Err(e)) => future::err(e),
            Err(oneshot::Canceled) => future::err(crate::Error::ClientControllerDied),
        });

        Box::pin(resp_rx)
    }

    /// Start request without request body (e. g. `GET` request).
    fn start_request_end_stream(
        &self,
        headers: Headers,
        body: Option<Bytes>,
        trailers: Option<Headers>,
    ) -> ClientResponseFuture3 {
        ClientResponseFuture3::new(
            self.start_request(headers, body, trailers, true)
                .and_then(move |(_sender, response)| response),
        )
    }

    /// Start HTTP/2 `GET` request.
    fn start_get(&self, path: &str, authority: &str) -> ClientResponseFuture3 {
        let headers = Headers::from_vec(vec![
            Header::new(PseudoHeaderName::Method, "GET"),
            Header::new(PseudoHeaderName::Path, path.to_owned()),
            // TODO: store authority in self
            Header::new(PseudoHeaderName::Authority, authority.to_owned()),
            Header::new(
                PseudoHeaderName::Scheme,
                self.internals().http_scheme.as_bytes(),
            ),
        ]);
        self.start_request_end_stream(headers, None, None)
    }

    /// Start HTTP/2 `POST` request with given request body.
    fn start_post(&self, path: &str, authority: &str, body: Bytes) -> ClientResponseFuture3 {
        let headers = Headers::from_vec(vec![
            Header::new(PseudoHeaderName::Method, "POST"),
            Header::new(PseudoHeaderName::Path, path.to_owned()),
            Header::new(PseudoHeaderName::Authority, authority.to_owned()),
            Header::new(
                PseudoHeaderName::Scheme,
                self.internals().http_scheme.as_bytes(),
            ),
        ]);
        self.start_request_end_stream(headers, Some(body), None)
    }

    /// Start `POST` request.
    ///
    /// This operation returns a sink which can be used to supply request body.
    fn start_post_sink(
        &self,
        path: &str,
        authority: &str,
    ) -> TryFutureBox<(SinkAfterHeadersBox, ClientResponseFutureImpl)> {
        let headers = Headers::from_vec(vec![
            Header::new(PseudoHeaderName::Method, "POST"),
            Header::new(PseudoHeaderName::Path, path.to_owned()),
            Header::new(PseudoHeaderName::Authority, authority.to_owned()),
            Header::new(
                PseudoHeaderName::Scheme,
                self.internals().http_scheme.as_bytes(),
            ),
        ]);
        self.start_request(headers, None, None, false)
    }
}
