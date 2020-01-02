#![allow(dead_code)]

use std::mem;
use std::sync::Arc;
use std::sync::Mutex;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use super::atomic_box_option::AtomicBoxOption;
use futures::task::Context;

struct WakerShared {
    waiters: Mutex<Vec<Arc<WaiterShared>>>,
}

pub struct Waker {
    shared: Arc<WakerShared>,
}

impl Waker {
    pub fn new() -> Waker {
        Waker {
            shared: Arc::new(WakerShared {
                waiters: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn wake_all(&self) {
        let mut lock = self.shared.waiters.lock().expect("lock");
        let waiters = mem::replace(&mut *lock, Vec::new());
        for waiter in waiters {
            debug_assert!(waiter.waker_knows.load(Ordering::Relaxed));
            waiter.waker_knows.store(false, Ordering::Relaxed);
            waiter.wake();
        }
    }

    pub fn new_waiter(&self) -> Waiter {
        Waiter {
            waker: self.shared.clone(),
            shared: Arc::new(WaiterShared {
                task: AtomicBoxOption::new(),
                waker_knows: AtomicBool::new(false),
            }),
        }
    }
}

pub struct Waiter {
    waker: Arc<WakerShared>,
    shared: Arc<WaiterShared>,
}

struct WaiterShared {
    task: AtomicBoxOption<std::task::Waker>,
    waker_knows: AtomicBool,
}

impl WaiterShared {
    fn wake(&self) {
        match self.task.swap_null(Ordering::SeqCst) {
            None => {}
            Some(task) => task.wake(),
        }
    }
}

impl Waiter {
    pub fn park(&self, context: &mut Context<'_>) {
        let mut lock = self.waker.waiters.lock().expect("lock");

        self.shared
            .task
            .store_box(Box::new(context.waker().clone()), Ordering::SeqCst);

        if self.shared.waker_knows.load(Ordering::Relaxed) {
            return;
        }

        self.shared.waker_knows.store(true, Ordering::Relaxed);
        lock.push(self.shared.clone());
    }
}
