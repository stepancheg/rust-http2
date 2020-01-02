use futures::stream::Stream;
use std::future::Future;

#[allow(dead_code)]
pub fn assert_send_future<O, F: Future<Output = O> + Send>(f: F) -> F {
    f
}
#[allow(dead_code)]
pub fn assert_send_stream<I, S: Stream<Item = I> + Send>(s: S) -> S {
    s
}
