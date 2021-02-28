use bytes::Bytes;

use crate::data_or_headers::DataOrHeaders;
use crate::data_or_headers_with_flag::DataOrHeadersWithFlag;
use crate::solicit::end_stream::EndStream;
use crate::solicit::header::Headers;

/// Stream frame content after initial headers
#[derive(Debug)]
pub enum DataOrTrailers {
    /// DATA frame
    Data(Bytes, EndStream),
    /// HEADERS frame with END_STREAM flag
    Trailers(Headers),
}

impl DataOrTrailers {
    pub fn intermediate_data(data: Bytes) -> Self {
        DataOrTrailers::Data(data, EndStream::No)
    }

    pub fn into_part(self) -> DataOrHeadersWithFlag {
        match self {
            DataOrTrailers::Data(data, end_stream) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Data(data),
                end_stream,
            },
            DataOrTrailers::Trailers(headers) => DataOrHeadersWithFlag {
                content: DataOrHeaders::Headers(headers),
                end_stream: EndStream::Yes,
            },
        }
    }

    pub fn end_stream(&self) -> EndStream {
        match self {
            DataOrTrailers::Data(_, end_stream) => *end_stream,
            DataOrTrailers::Trailers(_) => EndStream::Yes,
        }
    }
}
