use std::sync::Arc;

use futures::task::Task;
use futures::task;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;

use super::atomic_int_box::*;


enum State {
    Open = 0,
    Closed = 1,
    ControllerDead = 2,
}

const OPEN: u32 = State::Open as u32;
const CLOSED: u32 = State::Closed as u32;
const CONTROLLER_DEAD: u32 = State::ControllerDead as u32;

struct Shared {
    state: AtomicIntOrBox<Task>,
}

pub struct LatchController {
    shared: Arc<Shared>,
}

pub struct Latch {
    shared: Arc<Shared>,
}

pub fn latch() -> (LatchController, Latch) {
    let shared = Arc::new(Shared {
        state: AtomicIntOrBox::from_int(CLOSED),
    });
    (LatchController { shared: shared.clone() }, Latch { shared: shared })
}

impl LatchController {
    pub fn open(&self) {
        // fast track
        if let DecodedRef::Int(OPEN) = self.shared.state.load() {
            return;
        }

        if let DecodedBox::Box(task) = self.shared.state.swap(DecodedBox::Int(OPEN)) {
            task.notify();
        }
    }

    pub fn close(&self) {
        // fast track
        if let DecodedRef::Int(CLOSED) = self.shared.state.load() {
            return;
        }

        self.shared.state.store(DecodedBox::Int(CLOSED));

        // no need to unpark, because nobody is subscribed to close
    }
}

impl Drop for LatchController {
    fn drop(&mut self) {
        if let DecodedBox::Box(task) = self.shared.state.swap(DecodedBox::Int(CONTROLLER_DEAD)) {
            task.notify();
        }
    }
}

impl Stream for Latch {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let s = match self.shared.state.load() {
                DecodedRef::Int(OPEN) => return Ok(Async::Ready(Some(()))),
                DecodedRef::Int(CONTROLLER_DEAD) => return Err(()),
                s => s,
            };

            match self.shared.state.compare_exchange(s, DecodedBox::Box(Box::new(task::current()))) {
                Ok(_) => return Ok(Async::NotReady),
                _ => {}
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
