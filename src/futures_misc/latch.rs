use std::sync::Arc;

use futures::task::Task;
use futures::task;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;

use super::atomic_int_box::*;


const OPEN: U2 = U2::V0;
const CLOSED: U2 = U2::V1;
const CONTROLLER_DEAD: U2 = U2::V2;

struct Shared {
    state: AtomicU2OrBox<Task>,
}

pub struct LatchController {
    shared: Arc<Shared>,
}

pub struct Latch {
    shared: Arc<Shared>,
}

pub fn latch() -> (LatchController, Latch) {
    let shared = Arc::new(Shared {
        state: AtomicU2OrBox::from_u2(CLOSED),
    });
    (LatchController { shared: shared.clone() }, Latch { shared: shared })
}

impl LatchController {
    pub fn open(&self) {
        // fast track
        if let DecodedRef::U2(OPEN) = self.shared.state.load() {
            return;
        }

        if let DecodedBox::Box(task) = self.shared.state.swap(DecodedBox::U2(OPEN)) {
            task.notify();
        }
    }

    pub fn close(&self) {
        // fast track
        if let DecodedRef::U2(CLOSED) = self.shared.state.load() {
            return;
        }

        self.shared.state.store(DecodedBox::U2(CLOSED));

        // no need to unpark, because nobody is subscribed to close
    }
}

impl Drop for LatchController {
    fn drop(&mut self) {
        if let DecodedBox::Box(task) = self.shared.state.swap(DecodedBox::U2(CONTROLLER_DEAD)) {
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
                DecodedRef::U2(OPEN) => return Ok(Async::Ready(Some(()))),
                DecodedRef::U2(CONTROLLER_DEAD) => return Err(()),
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

    use futures::executor::Spawn;
    use futures::executor;
    use futures::Poll;

    use futures_misc::test::*;

    use super::*;

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
