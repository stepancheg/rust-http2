use std::sync::Arc;
use std::sync::Mutex;

use futures::task::Task;
use futures::task::park;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;


enum State {
    Open,
    Closed,
    ControllerDead,
}

struct Shared {
    state: State,
    task: Option<Task>,
}

pub struct LatchController {
    shared: Arc<Mutex<Shared>>,
}

pub struct Latch {
    shared: Arc<Mutex<Shared>>,
}

pub fn latch() -> (LatchController, Latch) {
    let shared = Arc::new(Mutex::new(Shared {
        state: State::Closed,
        task: None,
    }));
    (LatchController { shared: shared.clone() }, Latch { shared: shared })
}

impl LatchController {
    pub fn open(&self) {
        let mut guard = self.shared.lock().expect("lock");
        guard.state = State::Open;
        if let Some(task) = guard.task.take() {
            task.unpark();
        }
    }

    pub fn close(&self) {
        let mut guard = self.shared.lock().expect("lock");
        guard.state = State::Closed;
    }
}

impl Drop for LatchController {
    fn drop(&mut self) {
        let mut guard = self.shared.lock().expect("lock");
        guard.state = State::ControllerDead;
        if let Some(task) = guard.task.take() {
            task.unpark();
        }
    }
}

impl Stream for Latch {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut guard = self.shared.lock().expect("lock");
        match guard.state {
            State::Open => Ok(Async::Ready(Some(()))),
            State::ControllerDead => Err(()),
            State::Closed => {
                guard.task = Some(park());
                Ok(Async::NotReady)
            }
        }
    }
}
