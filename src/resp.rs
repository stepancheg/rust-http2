use futures::future;
use futures::future::Future;
use futures::stream;
use futures::stream::Stream;

use bytes::Bytes;

use solicit_async::*;
use solicit::header::Headers;
use message::SimpleHttpMessage;

use error::Error;

use data_or_trailers::*;
use data_or_headers::DataOrHeaders;
use data_or_headers_with_flag::DataOrHeadersWithFlag;
use data_or_headers_with_flag::DataOrHeadersWithFlagStream;


/// Convenient wrapper around async HTTP response future/stream
pub struct Response(pub HttpFutureSend<(Headers, HttpStreamAfterHeaders)>);

impl Response {
    // constructors

    pub fn new<F>(future: F) -> Response
        where F : Future<Item=(Headers, HttpStreamAfterHeaders), Error=Error> + Send + 'static
    {
        Response(Box::new(future))
    }

    pub fn headers_and_stream(headers: Headers, stream: HttpStreamAfterHeaders) -> Response
    {
        Response::new(future::ok((headers, stream)))
    }

    pub fn headers_and_bytes_stream<S>(headers: Headers, content: S) -> Response
        where S : Stream<Item=Bytes, Error=Error> + Send + 'static
    {
        Response::headers_and_stream(headers, HttpStreamAfterHeaders::bytes(content))
    }

    /// Create a response with only headers
    pub fn headers(headers: Headers) -> Response {
        Response::headers_and_bytes_stream(headers, stream::empty())
    }

    /// Create a response with headers and response body
    pub fn headers_and_bytes<B : Into<Bytes>>(header: Headers, content: B) -> Response {
        Response::headers_and_bytes_stream(header, stream::once(Ok(content.into())))
    }

    pub fn message(message: SimpleHttpMessage) -> Response {
        Response::headers_and_bytes(message.headers, message.body)
    }

    pub fn not_found_404() -> Response {
        Response::headers(Headers::not_found_404())
    }

    pub fn redirect_302(location: &str) -> Response {
        let mut headers = Headers::from_status(302);
        headers.add("location", location);
        Response::headers(headers)
    }

    pub fn from_stream<S>(stream: S) -> Response
        where S : Stream<Item=DataOrHeadersWithFlag, Error=Error> + Send + 'static
    {
        // Check that first frame is HEADERS
        Response::new(stream.into_future().map_err(|(p, _s)| p).and_then(|(first, rem)| {
            match first {
                Some(part) => {
                    match part.content {
                        DataOrHeaders::Headers(headers) => {
                            Ok((headers, HttpStreamAfterHeaders::from_parts(rem)))
                        },
                        DataOrHeaders::Data(..) => {
                            Err(Error::InvalidFrame("data before headers".to_owned()))
                        }
                    }
                }
                None => {
                    Err(Error::InvalidFrame("empty response, expecting headers".to_owned()))
                }
            }
        }))
    }

    pub fn err(err: Error) -> Response {
        Response::new(future::err(err))
    }

    // getters

    pub fn into_stream_flag(self) -> HttpFutureStreamSend<DataOrHeadersWithFlag> {
        Box::new(self.0.map(|(headers, rem)| {
            // NOTE: flag may be wrong for first item
            let header = stream::once(Ok(DataOrHeadersWithFlag::intermediate_headers(headers)));
            let rem = rem.into_flag_stream();
            header.chain(rem)
        }).flatten_stream())
    }

    pub fn into_stream(self) -> HttpFutureStreamSend<DataOrHeaders> {
        Box::new(self.into_stream_flag().map(|c| c.content))
    }

    pub fn into_part_stream(self) -> DataOrHeadersWithFlagStream {
        DataOrHeadersWithFlagStream::new(self.into_stream_flag())
    }

    pub fn collect(self) -> HttpFutureSend<SimpleHttpMessage> {
        Box::new(self.into_stream().fold(SimpleHttpMessage::new(), |mut c, p| {
            c.add(p);
            Ok::<_, Error>(c)
        }))
    }
}
