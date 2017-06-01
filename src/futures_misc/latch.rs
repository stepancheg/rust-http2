use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::task::Task;
use futures::task;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;


enum State {
    Open = 0,
    Closed = 1,
    ControllerDead = 2,
}

const OPEN: usize = State::Open as usize;
const CLOSED: usize = State::Closed as usize;
const CONTROLLER_DEAD: usize = State::ControllerDead as usize;

impl State {
    fn from(value: usize) -> State {
        match value {
            OPEN => State::Open,
            CLOSED => State::Closed,
            CONTROLLER_DEAD => State::ControllerDead,
            _ => unreachable!(),
        }
    }
}

struct Guarded {
    task: Option<Task>,
}

struct Shared {
    state: AtomicUsize,
    guarded: Mutex<Guarded>,
}

pub struct LatchController {
    shared: Arc<Shared>,
}

pub struct Latch {
    shared: Arc<Shared>,
}

pub fn latch() -> (LatchController, Latch) {
    let shared = Arc::new(Shared {
        state: AtomicUsize::new(CLOSED),
        guarded: Mutex::new(Guarded {
            task: None,
        }),
    });
    (LatchController { shared: shared.clone() }, Latch { shared: shared })
}

impl LatchController {
    pub fn open(&self) {
        if self.shared.state.load(Ordering::SeqCst) == OPEN as usize {
            return;
        }

        self.shared.state.store(OPEN, Ordering::SeqCst);

        let mut guard = self.shared.guarded.lock().expect("lock");
        if let Some(task) = guard.task.take() {
            task.notify();
        }
    }

    pub fn close(&self) {
        if self.shared.state.load(Ordering::SeqCst) == CLOSED as usize {
            return;
        }

        self.shared.state.store(CLOSED, Ordering::SeqCst);

        // no need to unpark, because nobody is subscribed to close
    }
}

impl Drop for LatchController {
    fn drop(&mut self) {
        self.shared.state.store(CONTROLLER_DEAD, Ordering::SeqCst);

        let mut guard = self.shared.guarded.lock().expect("lock");
        if let Some(task) = guard.task.take() {
            task.notify();
        }
    }
}

impl Stream for Latch {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let state = self.shared.state.load(Ordering::SeqCst);
        let state = State::from(state);

        match state {
            State::Open => Ok(Async::Ready(Some(()))),
            State::ControllerDead => Err(()),
            State::Closed => {
                let mut guard = self.shared.guarded.lock().expect("lock");
                guard.task = Some(task::current());
                Ok(Async::NotReady)
            }
        }
    }
}

#[cfg(test)]
mod test {

    use std::fmt::Debug;

    use futures::executor::NotifyHandle;
    use futures::executor::Notify;
    use futures::executor::Spawn;
    use futures::executor;
    use futures::Poll;

    use super::*;

    fn notify_noop() -> NotifyHandle {
        struct Noop;

        impl Notify for Noop {
            fn notify(&self, _id: usize) {}
        }

        const NOOP : &'static Noop = &Noop;

        NotifyHandle::from(NOOP)
    }

    fn assert_notify_stream<S>(
        expected: Poll<Option<S::Item>, S::Error>,
        stream: &mut Spawn<S>)
        where
            S : Stream,
            S::Item : Debug,
            S::Item : PartialEq,
            S::Error : Debug,
            S::Error : PartialEq,
    {
        assert_eq!(
            expected,
            stream.poll_stream_notify(&notify_noop(), 1));
    }

    #[test]
    fn test() {
        let (c, l) = latch();

        let mut l = executor::spawn(l);

        // initially closed
        assert_notify_stream(Ok(Async::NotReady), &mut l);
        assert_notify_stream(Ok(Async::NotReady), &mut l);

        c.open();

        assert_notify_stream(Ok(Async::Ready(Some(()))), &mut l);
        assert_notify_stream(Ok(Async::Ready(Some(()))), &mut l);

        c.close();

        assert_notify_stream(Ok(Async::NotReady), &mut l);

        c.close();

        assert_notify_stream(Ok(Async::NotReady), &mut l);
    }

}
