use std::sync::Arc;
use std::sync::Mutex;

use futures::Async;
use futures::Poll;

use futures::stream::Stream;
use futures::task::Task;
use futures::task;

enum State {
    Nothing,
    Signalled,
    SenderDead,
}

struct Shared {
    task: Option<Task>,
    state: State,
}

pub struct SignalSender {
    shared: Arc<Mutex<Shared>>,
}

pub struct SignalReceiver {
    shared: Arc<Mutex<Shared>>,
}

pub fn signal() -> (SignalSender, SignalReceiver) {
    let shared = Arc::new(Mutex::new(Shared {
        task: None,
        state: State::Nothing,
    }));

    (SignalSender { shared: shared.clone() }, SignalReceiver { shared: shared })
}

impl Drop for SignalSender {
    fn drop(&mut self) {
        let mut guard = self.shared.lock().expect("lock");
        guard.state = State::SenderDead;
        if let Some(task) = guard.task.take() {
            task.notify();
        }
    }
}

impl SignalSender {
    pub fn signal(&self) {
        let mut guard = self.shared.lock().expect("lock");
        guard.state = State::Signalled;
        if let Some(task) = guard.task.take() {
            task.notify();
        }
    }
}

impl Stream for SignalReceiver {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut guard = self.shared.lock().expect("lock");
        match guard.state {
            State::Signalled => {
                guard.state = State::Nothing;
                guard.task = None;
                Ok(Async::Ready(Some(())))
            }
            State::SenderDead => {
                guard.task = None;
                Err(())
            }
            State::Nothing => {
                guard.task = Some(task::current());
                Ok(Async::NotReady)
            }
        }
    }
}
