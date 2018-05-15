use std::sync::Arc;
use std::sync::Mutex;

use futures::future::Future;

use error;
use std::panic::AssertUnwindSafe;
use misc::any_to_string;

#[derive(Default, Clone)]
pub struct ClientDiedErrorHolder {
    error: Arc<Mutex<Option<Arc<error::Error>>>>,
}

impl ClientDiedErrorHolder {
    pub fn new() -> ClientDiedErrorHolder {
        Default::default()
    }

    pub fn error(&self) -> error::Error {
        let lock = self.error.lock().unwrap();
        error::Error::ClientDied((*lock).clone())
    }

    fn set_once(&self, error: error::Error) {
        let mut lock = self.error.lock().unwrap();
        if (*lock).is_none() {
            *lock = Some(Arc::new(error));
        }
    }

    pub fn wrap_future(&self, future: impl Future<Item=(), Error=error::Error>)
        -> impl Future<Item=(), Error=()>
    {

        let holder = self.clone();
        let future = future.map_err(move |e| {
            warn!("client completed with error: {:?}", e);
            holder.set_once(e);
            ()
        });

        let holder = self.clone();
        let future = AssertUnwindSafe(future).catch_unwind().then(move |r| {
            if let Err(e) = r {
                let message = any_to_string(e);
                warn!("client panicked: {}", message);
                holder.set_once(error::Error::ClientPanicked(message));
            }
            Ok(())
        });

        future
    }
}

