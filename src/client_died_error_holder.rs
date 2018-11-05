use std::marker;
use std::sync::Arc;
use std::sync::Mutex;

use futures::future::Future;

use error;
use misc::any_to_string;
use std::panic::AssertUnwindSafe;

pub trait DiedType: Default + Clone {
    fn what() -> &'static str;
}

#[derive(Copy, Clone, Default)]
pub struct ClientDiedType;
#[derive(Copy, Clone, Default)]
pub struct ConnDiedType;

impl DiedType for ClientDiedType {
    fn what() -> &'static str {
        "client"
    }
}

impl DiedType for ConnDiedType {
    fn what() -> &'static str {
        "connection"
    }
}

#[derive(Default, Clone)]
pub(crate) struct SomethingDiedErrorHolder<D: DiedType> {
    error: Arc<Mutex<Option<Arc<error::Error>>>>,
    _marker: marker::PhantomData<D>,
}

impl<D: DiedType> SomethingDiedErrorHolder<D> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn client_died_error(&self) -> Option<Arc<error::Error>> {
        let lock = self.error.lock().unwrap();
        (*lock).clone()
    }

    pub fn error(&self) -> error::Error {
        error::Error::ClientDied(self.client_died_error())
    }

    fn set_once(&self, error: error::Error) {
        let mut lock = self.error.lock().unwrap();
        if (*lock).is_none() {
            *lock = Some(Arc::new(error));
        }
    }

    pub fn wrap_future(
        &self,
        future: impl Future<Item = (), Error = error::Error>,
    ) -> impl Future<Item = (), Error = ()> {
        let holder = self.clone();
        let future = future.then(move |r| {
            match r {
                Ok(()) => {
                    info!("{} completed without errors", D::what());
                    holder.set_once(error::Error::ClientCompletedWithoutError);
                }
                Err(e) => {
                    warn!("{} completed with error: {:?}", D::what(), e);
                    holder.set_once(e);
                }
            }
            Ok::<(), ()>(())
        });

        let holder = self.clone();
        let future = AssertUnwindSafe(future).catch_unwind().then(move |r| {
            if let Err(e) = r {
                let message = any_to_string(e);
                warn!("{} panicked: {}", D::what(), message);
                holder.set_once(error::Error::ClientPanicked(message));
            }
            Ok(())
        });

        future
    }
}
