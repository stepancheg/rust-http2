use std::future::Future;
use std::pin::Pin;
use tokio::runtime::Handle;

pub trait Executor {
    fn execute(&self, f: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
}

impl Executor for Handle {
    fn execute(&self, f: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
        self.spawn(f);
    }
}
