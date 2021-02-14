use futures::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

/// Similar to `select`, but does not require `Unpin`
#[allow(dead_code)]
pub(crate) fn select_lite<A, B>(a: A, b: B) -> SelectLite<A, B> {
    SelectLite { a, b }
}

pub(crate) struct SelectLite<A, B> {
    a: A,
    b: B,
}

impl<A: Future, B: Future<Output = A::Output>> Future for SelectLite<A, B> {
    type Output = A::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let self_mut = self.get_unchecked_mut();
            if let Poll::Ready(a) = Pin::new_unchecked(&mut self_mut.a).poll(cx) {
                return Poll::Ready(a);
            }
            if let Poll::Ready(b) = Pin::new_unchecked(&mut self_mut.b).poll(cx) {
                return Poll::Ready(b);
            }
            Poll::Pending
        }
    }
}
