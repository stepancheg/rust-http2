use solicit::frame::headers::HeadersDecodedFrame;
use solicit::frame::*;
use solicit::stream_id::StreamId;

/// Frames with stream
#[derive(Debug)]
pub enum HttpFrameStream {
    Data(DataFrame),
    Headers(HeadersDecodedFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    PushPromise(PushPromiseFrame),
    WindowUpdate(WindowUpdateFrame),
}

impl HttpFrameStream {
    #[allow(dead_code)]
    pub fn into_frame(self) -> HttpFrameDecoded {
        match self {
            HttpFrameStream::Headers(f) => HttpFrameDecoded::Headers(f),
            HttpFrameStream::Data(f) => HttpFrameDecoded::Data(f),
            HttpFrameStream::Priority(f) => HttpFrameDecoded::Priority(f),
            HttpFrameStream::WindowUpdate(f) => HttpFrameDecoded::WindowUpdate(f),
            HttpFrameStream::RstStream(f) => HttpFrameDecoded::RstStream(f),
            HttpFrameStream::PushPromise(f) => HttpFrameDecoded::PushPromise(f),
        }
    }

    #[allow(dead_code)]
    pub fn get_stream_id(&self) -> StreamId {
        match self {
            &HttpFrameStream::Data(ref f) => f.get_stream_id(),
            &HttpFrameStream::Headers(ref f) => f.get_stream_id(),
            &HttpFrameStream::Priority(ref f) => f.get_stream_id(),
            &HttpFrameStream::WindowUpdate(ref f) => f.get_stream_id(),
            &HttpFrameStream::RstStream(ref f) => f.get_stream_id(),
            &HttpFrameStream::PushPromise(ref f) => f.get_stream_id(),
        }
    }

    #[allow(dead_code)]
    pub fn is_end_of_stream(&self) -> bool {
        match self {
            &HttpFrameStream::Headers(ref f) => f.is_end_of_stream(),
            &HttpFrameStream::Data(ref f) => f.is_end_of_stream(),
            &HttpFrameStream::Priority(..) => false,
            &HttpFrameStream::WindowUpdate(..) => false,
            &HttpFrameStream::RstStream(..) => true,
            &HttpFrameStream::PushPromise(..) => false,
        }
    }
}

/// Frames without stream (zero stream id)
#[derive(Debug)]
pub enum HttpFrameConn {
    Settings(SettingsFrame),
    Ping(PingFrame),
    Goaway(GoawayFrame),
    WindowUpdate(WindowUpdateFrame),
}

impl HttpFrameConn {
    #[allow(dead_code)]
    pub fn into_frame(self) -> HttpFrame {
        match self {
            HttpFrameConn::Settings(f) => HttpFrame::Settings(f),
            HttpFrameConn::Ping(f) => HttpFrame::Ping(f),
            HttpFrameConn::Goaway(f) => HttpFrame::Goaway(f),
            HttpFrameConn::WindowUpdate(f) => HttpFrame::WindowUpdate(f),
        }
    }
}

#[derive(Debug)]
pub enum HttpFrameClassified {
    Stream(HttpFrameStream),
    Conn(HttpFrameConn),
    Unknown(RawFrame),
}

impl HttpFrameClassified {
    pub fn from(frame: HttpFrameDecoded) -> Self {
        match frame {
            HttpFrameDecoded::Data(f) => HttpFrameClassified::Stream(HttpFrameStream::Data(f)),
            HttpFrameDecoded::Headers(f) => {
                HttpFrameClassified::Stream(HttpFrameStream::Headers(f))
            }
            HttpFrameDecoded::Priority(f) => {
                HttpFrameClassified::Stream(HttpFrameStream::Priority(f))
            }
            HttpFrameDecoded::RstStream(f) => {
                HttpFrameClassified::Stream(HttpFrameStream::RstStream(f))
            }
            HttpFrameDecoded::Settings(f) => HttpFrameClassified::Conn(HttpFrameConn::Settings(f)),
            HttpFrameDecoded::PushPromise(f) => {
                HttpFrameClassified::Stream(HttpFrameStream::PushPromise(f))
            }
            HttpFrameDecoded::Ping(f) => HttpFrameClassified::Conn(HttpFrameConn::Ping(f)),
            HttpFrameDecoded::Goaway(f) => HttpFrameClassified::Conn(HttpFrameConn::Goaway(f)),
            HttpFrameDecoded::WindowUpdate(f) => {
                if f.get_stream_id() != 0 {
                    HttpFrameClassified::Stream(HttpFrameStream::WindowUpdate(f))
                } else {
                    HttpFrameClassified::Conn(HttpFrameConn::WindowUpdate(f))
                }
            }
            HttpFrameDecoded::Unknown(f) => HttpFrameClassified::Unknown(f),
        }
    }
}
