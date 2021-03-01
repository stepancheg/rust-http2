/// An enum indicating whether the `HttpConnection` send operation should end the stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EndStream {
    /// The stream is closed/should be closed.
    Yes,
    /// The stream is open/should be kept open.
    No,
}

impl EndStream {
    /// Convert from boolean.
    pub fn from_bool(end_stream: bool) -> EndStream {
        match end_stream {
            true => EndStream::Yes,
            false => EndStream::No,
        }
    }
}
