use std::sync::Arc;

use futures::task::Task;
use futures::task;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;

use void::Void;

use super::atomic_int_box::*;


const OPEN: U2 = U2::V0;
const CLOSED_NO_WAIT: U2 = U2::V1;
const CONTROLLER_DEAD: U2 = U2::V2;

pub struct Controller {
    shared: Arc<Shared>,
}

pub struct Latch {
    shared: Arc<Shared>,
}

pub fn latch() -> (Controller, Latch) {
    let shared = Arc::new(Shared {
        state: AtomicU2OrBox::from_u2(CLOSED_NO_WAIT),
    });
    (Controller { shared: shared.clone() }, Latch { shared: shared })
}


struct Shared {
    state: AtomicU2OrBox<Task>,
}

impl Shared {
    fn is_open(&self) -> bool {
        match self.state.load() {
            DecodedRef::Ptr(_) => true,
            DecodedRef::U2(OPEN) => true,
            DecodedRef::U2(CLOSED_NO_WAIT) => false,
            DecodedRef::U2(CONTROLLER_DEAD) => true,
            DecodedRef::U2(_) => unreachable!(),
        }
    }
}

impl Controller {
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
        loop {
            let state = self.shared.state.load();
            match state {
                DecodedRef::U2(CLOSED_NO_WAIT) | DecodedRef::Ptr(_) => return,
                DecodedRef::U2(CONTROLLER_DEAD) => unreachable!(),
                DecodedRef::U2(OPEN) => {
                    if let Ok(_) = self.shared.state.compare_int_exchange(OPEN, DecodedBox::U2(CLOSED_NO_WAIT)) {
                        return;
                    }
                }
                DecodedRef::U2(_) => unreachable!(),
            }
        }

        // no need to unpark, because nobody is subscribed to close
    }

    /// For test
    pub fn is_open(&self) -> bool {
        self.shared.is_open()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let DecodedBox::Box(task) = self.shared.state.swap(DecodedBox::U2(CONTROLLER_DEAD)) {
            task.notify();
        }
    }
}

pub struct ControllerDead;

impl Latch {
    pub fn poll_ready(&self) -> Poll<(), ControllerDead> {
        loop {
            let s = match self.shared.state.load() {
                DecodedRef::U2(OPEN) => return Ok(Async::Ready(())),
                DecodedRef::U2(CONTROLLER_DEAD) => return Err(ControllerDead),
                s => s,
            };

            match self.shared.state.compare_exchange(s, DecodedBox::Box(Box::new(task::current()))) {
                Ok(_) => return Ok(Async::NotReady),
                _ => {}
            }
        }
    }
}

impl Stream for Latch {
    type Item = ();
    type Error = Void;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.poll_ready() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(())) => Ok(Async::Ready(Some(()))),
            Err(ControllerDead) => Ok(Async::Ready(None)),
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
