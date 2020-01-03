use void::Void;

use futures::future::Future;
use tokio_core::reactor;

pub trait Executor {
    fn execute(&self, f: Box<dyn Future<Item = (), Error = Void> + Send + 'static>);
}

impl Executor for reactor::Handle {
    fn execute(&self, f: Box<dyn Future<Item = (), Error = Void> + Send + 'static>) {
        self.spawn(f.map_err(|e| match e {}));
    }
}
