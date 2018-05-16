use solicit::StreamId;
use result::Result;
use solicit::frame::*;
use solicit::connection::HttpFrame;


/// Frames with stream
#[derive(Debug)]
pub enum HttpFrameStream {
    Data(DataFrame),
    Headers(HeadersFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    PushPromise(PushPromiseFrame),
    WindowUpdate(WindowUpdateFrame),
    Continuation(ContinuationFrame),
}

impl HttpFrameStream {
    #[allow(dead_code)]
    pub fn into_frame(self) -> HttpFrame {
        match self {
            HttpFrameStream::Headers(f) => HttpFrame::Headers(f),
            HttpFrameStream::Data(f) => HttpFrame::Data(f),
            HttpFrameStream::Priority(f) => HttpFrame::Priority(f),
            HttpFrameStream::WindowUpdate(f) => HttpFrame::WindowUpdate(f),
            HttpFrameStream::RstStream(f) => HttpFrame::RstStream(f),
            HttpFrameStream::PushPromise(f) => HttpFrame::PushPromise(f),
            HttpFrameStream::Continuation(f) => HttpFrame::Continuation(f),
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
            &HttpFrameStream::Continuation(ref f) => f.get_stream_id(),
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
            &HttpFrameStream::Continuation(..) => panic!("end of stream is defined in HEADERS"),
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
    pub fn from(frame: HttpFrame) -> Self {
        match frame {
            HttpFrame::Data(f) => HttpFrameClassified::Stream(HttpFrameStream::Data(f)),
            HttpFrame::Headers(f) => HttpFrameClassified::Stream(HttpFrameStream::Headers(f)),
            HttpFrame::Priority(f) => HttpFrameClassified::Stream(HttpFrameStream::Priority(f)),
            HttpFrame::RstStream(f) => HttpFrameClassified::Stream(HttpFrameStream::RstStream(f)),
            HttpFrame::Settings(f) => HttpFrameClassified::Conn(HttpFrameConn::Settings(f)),
            HttpFrame::PushPromise(f) => HttpFrameClassified::Stream(HttpFrameStream::PushPromise(f)),
            HttpFrame::Ping(f) => HttpFrameClassified::Conn(HttpFrameConn::Ping(f)),
            HttpFrame::Goaway(f) => HttpFrameClassified::Conn(HttpFrameConn::Goaway(f)),
            HttpFrame::WindowUpdate(f) => {
                if f.get_stream_id() != 0 {
                    HttpFrameClassified::Stream(HttpFrameStream::WindowUpdate(f))
                } else {
                    HttpFrameClassified::Conn(HttpFrameConn::WindowUpdate(f))
                }
            },
            HttpFrame::Continuation(f) => HttpFrameClassified::Stream(HttpFrameStream::Continuation(f)),
            HttpFrame::Unknown(f) => HttpFrameClassified::Unknown(f),
        }
    }

    pub fn _from_raw(raw_frame: &RawFrame) -> Result<HttpFrameClassified> {
        Ok(HttpFrameClassified::from(HttpFrame::from_raw(raw_frame)?))
    }
}

