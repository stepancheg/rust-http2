use futures::task::Context;
use futures::task::RawWaker;
use futures::task::RawWakerVTable;
use futures::task::Waker;
use std::ptr;

pub struct NopRuntime {
    waker: Waker,
}

static VTABLE: RawWakerVTable =
    RawWakerVTable::new(|_data| new_raw_waker(), |_data| {}, |_data| {}, |_data| {});

fn new_raw_waker() -> RawWaker {
    RawWaker::new(ptr::null(), &VTABLE)
}

impl NopRuntime {
    pub fn new() -> NopRuntime {
        NopRuntime {
            waker: unsafe { Waker::from_raw(new_raw_waker()) },
        }
    }

    pub fn context(&self) -> Context<'_> {
        Context::from_waker(&self.waker)
    }
}
