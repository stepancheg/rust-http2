#![allow(dead_code)]

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::task;
use futures::task::Task;

use futures::Async;
use futures::Poll;

use super::atomic_box_option::AtomicBoxOption;

use super::waiters::*;

struct ConnOutWindowShared {
    window_size: AtomicIsize,
    closed: AtomicBool,
}

struct StreamWindowShared {
    conn: Arc<ConnOutWindowShared>,
    task: AtomicBoxOption<Task>,
    closed: AtomicBool,
    window_size: AtomicIsize,
}

pub struct ConnOutWindowSender {
    waker: Waker,
    shared: Arc<ConnOutWindowShared>,
}

impl Drop for ConnOutWindowSender {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        self.waker.wake_all();
    }
}

struct ConnOutWindowReceiver {
    shared: Arc<ConnOutWindowShared>,
}

pub struct StreamOutWindowSender {
    shared: Arc<StreamWindowShared>,
}

impl Drop for StreamOutWindowSender {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        if let Some(task) = self.shared.task.swap_null(Ordering::SeqCst) {
            task.notify();
        }
    }
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
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub fn new_stream(&self, initial: u32) -> (StreamOutWindowSender, StreamOutWindowReceiver) {
        let shared = Arc::new(StreamWindowShared {
            conn: self.shared.clone(),
            window_size: AtomicIsize::new(initial as isize),
            task: AtomicBoxOption::new(),
            closed: AtomicBool::new(false),
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
        let old_size = self
            .shared
            .window_size
            .fetch_add(size as isize, Ordering::SeqCst) as i32;
        let new_size = old_size + size as i32;

        if new_size >= 0 {
            self.waker.wake_all();
        }
    }
}

impl StreamOutWindowSender {
    pub fn increase(&self, size: i32) {
        let old_size = self
            .shared
            .window_size
            .fetch_add(size as isize, Ordering::SeqCst) as i32;
        let new_size = old_size + size;
        if new_size >= 0 {
            if let Some(task) = self.shared.task.swap_null(Ordering::SeqCst) {
                task.notify();
            }
        }
    }
}

struct ConnDead;

pub enum StreamDead {
    Stream,
    Conn,
}

impl From<ConnDead> for StreamDead {
    fn from(_: ConnDead) -> StreamDead {
        StreamDead::Conn
    }
}

impl StreamOutWindowReceiver {
    pub fn decrease(&self, size: usize) {
        self.shared
            .conn
            .window_size
            .fetch_sub(size as isize, Ordering::SeqCst);
        self.shared
            .window_size
            .fetch_sub(size as isize, Ordering::SeqCst);
    }

    fn check_conn_closed(&self) -> Result<(), ConnDead> {
        if self.shared.conn.closed.load(Ordering::Relaxed) {
            Err(ConnDead)
        } else {
            Ok(())
        }
    }

    fn check_stream_closed(&self) -> Result<(), StreamDead> {
        self.check_conn_closed()?;

        if self.shared.closed.load(Ordering::SeqCst) {
            Err(StreamDead::Stream)
        } else {
            Ok(())
        }
    }

    fn poll_conn(&self) -> Poll<(), ConnDead> {
        self.check_conn_closed()?;

        if self.shared.conn.window_size.load(Ordering::SeqCst) >= 0 {
            return Ok(Async::Ready(()));
        }

        self.conn_waiter.park();

        self.check_conn_closed()?;

        Ok(
            if self.shared.conn.window_size.load(Ordering::SeqCst) >= 0 {
                Async::Ready(())
            } else {
                Async::NotReady
            },
        )
    }

    pub fn poll(&self) -> Poll<(), StreamDead> {
        self.check_stream_closed()?;

        if self.shared.window_size.load(Ordering::SeqCst) < 0 {
            self.shared
                .task
                .store_box(Box::new(task::current()), Ordering::SeqCst);

            self.check_stream_closed()?;

            if self.shared.window_size.load(Ordering::SeqCst) < 0 {
                return Ok(Async::NotReady);
            }
        }

        Ok(self.poll_conn()?)
    }
}
