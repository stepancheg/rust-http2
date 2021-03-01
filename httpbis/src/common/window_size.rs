#![allow(dead_code)]

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use std::task::Poll;

use super::atomic_box_option::AtomicBoxOption;

use super::waiters::*;
use crate::debug_undebug::DebugUndebug;
use futures::future;
use futures::task::Context;
use std::fmt;

#[derive(Debug)]
struct ConnOutWindowShared {
    window_size: AtomicIsize,
    closed: AtomicBool,
}

struct StreamWindowShared {
    conn: Arc<ConnOutWindowShared>,
    task: AtomicBoxOption<std::task::Waker>,
    closed: AtomicBool,
    window_size: AtomicIsize,
}

impl fmt::Debug for StreamWindowShared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let StreamWindowShared {
            conn,
            task,
            closed,
            window_size,
        } = self;
        f.debug_struct("StreamWindowShared")
            .field("conn", conn)
            .field("task", &DebugUndebug(task))
            .field("closed", closed)
            .field("window_size", window_size)
            .finish()
    }
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
            task.wake();
        }
    }
}

pub struct StreamOutWindowReceiver {
    conn_waiter: Waiter,
    shared: Arc<StreamWindowShared>,
}

impl fmt::Debug for StreamOutWindowReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamOutWindowReceiver")
            .field("conn_waiter", &"...")
            .field("shared", &self.shared)
            .finish()
    }
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

    pub fn get(&self) -> isize {
        self.shared.window_size.load(Ordering::SeqCst) as isize
    }

    pub fn increase(&self, size: usize) {
        assert!(size <= isize::max_value() as usize);
        let old_size = self
            .shared
            .window_size
            .fetch_add(size as isize, Ordering::SeqCst);
        let new_size = old_size + size as isize;

        if new_size > 0 {
            self.waker.wake_all();
        }
    }
}

impl StreamOutWindowSender {
    /// `size` can be negative when INITIAL_WINDOW_SIZE
    /// setting changes to lower value.
    pub fn increase(&self, size: isize) {
        let old_size = self
            .shared
            .window_size
            .fetch_add(size as isize, Ordering::SeqCst);
        let new_size = old_size + size as isize;
        if new_size > 0 {
            if let Some(task) = self.shared.task.swap_null(Ordering::SeqCst) {
                task.wake();
            }
        }
    }

    pub fn get(&self) -> isize {
        self.shared.window_size.load(Ordering::SeqCst) as isize
    }
}

struct ConnDead;

#[derive(Eq, PartialEq, Debug)]
pub(crate) enum StreamDead {
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

    fn check_conn_closed(&self) -> crate::Result<()> {
        if self.shared.conn.closed.load(Ordering::Relaxed) {
            // TODO: better error
            Err(crate::Error::ClientControllerDied)
        } else {
            Ok(())
        }
    }

    fn check_stream_closed(&self) -> crate::Result<()> {
        self.check_conn_closed()?;

        if self.shared.closed.load(Ordering::SeqCst) {
            // TODO: better error
            Err(crate::Error::ClientControllerDied)
        } else {
            Ok(())
        }
    }

    fn poll_conn(&self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        self.check_conn_closed()?;

        if self.shared.conn.window_size.load(Ordering::SeqCst) > 0 {
            return Poll::Ready(Ok(()));
        }

        self.conn_waiter.park(cx);

        self.check_conn_closed()?;

        if self.shared.conn.window_size.load(Ordering::SeqCst) > 0 {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    pub fn poll(&self, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        self.check_stream_closed()?;

        if self.shared.window_size.load(Ordering::SeqCst) <= 0 {
            self.shared
                .task
                .store_box(Box::new(cx.waker().clone()), Ordering::SeqCst);

            self.check_stream_closed()?;

            if self.shared.window_size.load(Ordering::SeqCst) <= 0 {
                return Poll::Pending;
            }
        }

        self.poll_conn(cx).map_err(|e| e.into())
    }

    pub async fn poll_f(&self) -> crate::Result<()> {
        future::poll_fn(|cx| self.poll(cx)).await
    }
}
