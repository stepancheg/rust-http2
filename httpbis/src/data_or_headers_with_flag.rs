use bytes::Bytes;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_trailers::DataOrTrailers;
use crate::solicit::end_stream::EndStream;
use crate::solicit::header::Headers;

/// Stream frame content with END_STREAM flag
#[derive(Debug)]
pub struct DataOrHeadersWithFlag {
    pub content: DataOrHeaders,
    pub end_stream: EndStream,
}

impl DataOrHeadersWithFlag {
    pub fn last_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            end_stream: EndStream::Yes,
        }
    }

    pub fn intermediate_headers(headers: Headers) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Headers(headers),
            end_stream: EndStream::No,
        }
    }

    pub fn intermediate_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            end_stream: EndStream::No,
        }
    }

    pub fn last_data(data: Bytes) -> Self {
        DataOrHeadersWithFlag {
            content: DataOrHeaders::Data(data),
            end_stream: EndStream::Yes,
        }
    }

    pub fn into_after_headers(self) -> DataOrTrailers {
        let DataOrHeadersWithFlag {
            content,
            end_stream,
        } = self;
        match content {
            DataOrHeaders::Data(data) => DataOrTrailers::Data(data, end_stream),
            DataOrHeaders::Headers(headers) => DataOrTrailers::Trailers(headers),
        }
    }
}

impl From<DataOrTrailers> for DataOrHeadersWithFlag {
    fn from(d: DataOrTrailers) -> Self {
        match d {
            DataOrTrailers::Data(data, end_stream) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                end_stream,
            },
            DataOrTrailers::Trailers(trailers) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(trailers),
                end_stream: EndStream::Yes,
            },
        }
    }
}
