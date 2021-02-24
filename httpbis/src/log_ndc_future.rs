use futures::task::Poll;
use futures::Future;
use log_ndc::Ndc;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;

pub(crate) trait ToNdc {
    fn to_ndc(&self) -> Ndc;
}

impl ToNdc for Arc<String> {
    fn to_ndc(&self) -> Ndc {
        Ndc::Arc(self.clone())
    }
}

pub(crate) fn log_ndc_future<N, F>(ndc: N, f: F) -> LogNdcFuture<N, F>
where
    N: ToNdc,
    F: Future,
{
    LogNdcFuture { ndc, f }
}

pub(crate) struct LogNdcFuture<N, F>
where
    N: ToNdc,
    F: Future,
{
    ndc: N,
    f: F,
}

impl<N, F> Future for LogNdcFuture<N, F>
where
    N: ToNdc,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let _guard = log_ndc::push(self.ndc.to_ndc());
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().f) }.poll(cx)
    }
}
