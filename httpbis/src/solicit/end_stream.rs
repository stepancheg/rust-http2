/// An enum indicating whether the `HttpConnection` send operation should end the stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EndStream {
    /// The stream should be closed
    Yes,
    /// The stream should still be kept open
    No,
}
