use solicit::header::Headers;
use data_or_trailers::HttpStreamAfterHeaders;
use resp::Response;


/// Central HTTP/2 service interface.
///
/// This trait is used by both client and server.
///
/// Client API simply implements this trait.
///
/// Server implementation calls implementation of this trait providede by user.
pub trait Service : Send + Sync + 'static {
    /// Start HTTP/2 request.
    ///
    /// `headers` param specifies initial request headers.
    /// `req` param contains asynchronous stream of request content,
    /// stream of zero or more `DATA` frames followed by optional
    /// trailer `HEADERS` frame.
    fn start_request(&self, headers: Headers, req: HttpStreamAfterHeaders) -> Response;
}
