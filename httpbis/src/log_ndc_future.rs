use futures::task::Poll;
use futures::Future;
use log_ndc::Ndc;
use std::pin::Pin;
use std::task::Context;

pub(crate) fn log_ndc_future<N, F>(ndc: N, f: F) -> LogNdcFuture<N, F>
where
    N: Into<Ndc> + Clone,
    F: Future,
{
    LogNdcFuture { ndc, f }
}

pub(crate) struct LogNdcFuture<N, F>
where
    N: Into<Ndc>,
    F: Future,
{
    ndc: N,
    f: F,
}

impl<N, F> Future for LogNdcFuture<N, F>
where
    N: Into<Ndc> + Clone,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let _guard = log_ndc::push(self.ndc.clone().into());
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().f) }.poll(cx)
    }
}
