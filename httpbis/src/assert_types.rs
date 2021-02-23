use futures::Stream;
use std::future::Future;

#[allow(dead_code)]
pub fn assert_send<T: Send>() {}
#[allow(dead_code)]
pub fn assert_sync<T: Sync>() {}
#[allow(dead_code)]
pub fn assert_unpin<T: Unpin>() {}

#[allow(dead_code)]
pub fn assert_send_value<T: Send>(t: T) -> T {
    t
}

#[allow(dead_code)]
pub fn assert_send_future<O, F: Future<Output = O> + Send>(f: F) -> F {
    f
}
#[allow(dead_code)]
pub fn assert_send_stream<I, S: Stream<Item = I> + Send>(s: S) -> S {
    s
}

#[allow(dead_code)]
pub fn assert_future_output<O, F>(f: F) -> F
where
    F: Future<Output = O>,
{
    f
}
