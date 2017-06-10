use void::Void;

use futures::future::Future;
use futures_cpupool::CpuPool;
use tokio_core::reactor;

pub trait Executor {
    fn execute(&self, f: Box<Future<Item=(), Error=Void> + 'static>);
}

impl Executor for CpuPool {
    fn execute(&self, _f: Box<Future<Item=(), Error=Void> + 'static>) {
        unimplemented!()
        //self.spawn(f).forget();
    }
}

impl Executor for reactor::Handle {
    fn execute(&self, f: Box<Future<Item=(), Error=Void> + 'static>) {
        self.spawn(f.map_err(|e| match e {}));
    }
}

// Where execute requests on client and responses on server
pub enum CpuPoolOption {
    // Execute in event loop
    Inline,
    // Execute in provided CpuPool
    CpuPool(CpuPool),
}

impl CpuPoolOption {
    pub(crate) fn make_executor(&self, lh: &reactor::Handle) -> Box<Executor> {
        match self {
            &CpuPoolOption::Inline => Box::new(lh.clone()),
            &CpuPoolOption::CpuPool(ref pool) => Box::new(pool.clone()),
        }
    }
}
