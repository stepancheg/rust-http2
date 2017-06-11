#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering;

use futures::task;
use futures::task::Task;

use super::atomic_box_option::AtomicBoxOption;

use futures::Async;

use super::waiters::*;



struct ConnOutWindowShared {
    window_size: AtomicIsize,
}

struct StreamWindowShared {
    conn: Arc<ConnOutWindowShared>,
    task: AtomicBoxOption<Task>,
    window_size: AtomicIsize,
}


pub struct ConnOutWindowSender {
    waker: Waker,
    shared: Arc<ConnOutWindowShared>,
}

struct ConnOutWindowReceiver {
    shared: Arc<ConnOutWindowShared>,
}

pub struct StreamOutWindowSender {
    shared: Arc<StreamWindowShared>,
}

pub struct StreamOutWindowReceiver {
    conn_waiter: Waiter,
    shared: Arc<StreamWindowShared>,
}


impl ConnOutWindowSender {
    pub fn new(size: u32) -> ConnOutWindowSender {
        ConnOutWindowSender {
            waker: Waker::new(),
            shared: Arc::new(ConnOutWindowShared {
                window_size: AtomicIsize::new(size as isize),
            }),
        }
    }

    pub fn new_stream(&self, initial: u32) -> (StreamOutWindowSender, StreamOutWindowReceiver) {
        let shared = Arc::new(StreamWindowShared {
            conn: self.shared.clone(),
            window_size: AtomicIsize::new(initial as isize),
            task: AtomicBoxOption::new(),
        });

        let sender = StreamOutWindowSender {
            shared: shared.clone(),
        };
        let receiver = StreamOutWindowReceiver {
            conn_waiter: self.waker.new_waiter(),
            shared: shared,
        };
        (sender, receiver)
    }

    pub fn increase(&self, size: u32) {
        let old_size = self.shared.window_size.fetch_add(size as isize, Ordering::SeqCst) as i32;
        let new_size = old_size + size as i32;

        if new_size >= 0 {
            self.waker.wake_all();
        }
    }
}

impl StreamOutWindowSender {
    pub fn increase(&self, size: i32) {
        let old_size = self.shared.window_size.fetch_add(size as isize, Ordering::SeqCst) as i32;
        let new_size = old_size + size;
        if new_size >= 0 {
            if let Some(task) = self.shared.task.swap_null(Ordering::SeqCst) {
                task.notify();
            }
        }
    }
}

impl StreamOutWindowReceiver {
    pub fn decrease(&self, size: u32) {
        self.shared.conn.window_size.fetch_sub(size as isize, Ordering::SeqCst);
        self.shared.window_size.fetch_sub(size as isize, Ordering::SeqCst);
    }

    fn poll_conn(&self) -> Async<()> {
        if self.shared.conn.window_size.load(Ordering::SeqCst) >= 0 {
            return Async::Ready(());
        }

        self.conn_waiter.park();

        if self.shared.conn.window_size.load(Ordering::SeqCst) >= 0 {
            Async::Ready(())
        } else {
            Async::NotReady
        }
    }

    pub fn poll(&self) -> Async<()> {
        if self.shared.window_size.load(Ordering::SeqCst) < 0 {

            self.shared.task.store_box(Box::new(task::current()), Ordering::SeqCst);

            if self.shared.window_size.load(Ordering::SeqCst) < 0 {
                return Async::NotReady;
            }
        }

        self.poll_conn()
    }
}
