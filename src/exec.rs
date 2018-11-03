use std::fmt;

use void::Void;

use futures::future::Future;
use futures_cpupool::CpuPool;
use tokio_core::reactor;

pub trait Executor {
    fn execute(&self, f: Box<Future<Item = (), Error = Void> + Send + 'static>);
}

impl Executor for CpuPool {
    fn execute(&self, f: Box<Future<Item = (), Error = Void> + Send + 'static>) {
        self.spawn(f).forget();
    }
}

impl Executor for reactor::Handle {
    fn execute(&self, f: Box<Future<Item = (), Error = Void> + Send + 'static>) {
        self.spawn(f.map_err(|e| match e {}));
    }
}

// Where execute requests on client and responses on server
#[derive(Clone)]
pub enum _CpuPoolOption {
    // Execute in event loop
    SingleThread,
    // Execute in provided CpuPool
    CpuPool(CpuPool),
}

impl fmt::Debug for _CpuPoolOption {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &_CpuPoolOption::SingleThread => write!(f, "SingleThread"),
            &_CpuPoolOption::CpuPool(..) => write!(f, "CpuPool"),
        }
    }
}

impl _CpuPoolOption {
    pub(crate) fn _make_executor(&self, lh: &reactor::Handle) -> Box<Executor> {
        match self {
            &_CpuPoolOption::SingleThread => Box::new(lh.clone()),
            &_CpuPoolOption::CpuPool(ref pool) => Box::new(pool.clone()),
        }
    }
}
