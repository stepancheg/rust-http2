use std::marker;
use std::sync::Arc;
use std::sync::Mutex;

use futures::future;

use std::future::Future;

use crate::error;
use crate::misc::any_to_string;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

pub(crate) trait DiedType: Default + Clone + Unpin + Send {
    fn what() -> &'static str;

    fn wrap_error(e: Arc<crate::Error>) -> crate::Error;
}

#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct ClientDiedType;
#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct ConnDiedType;

impl DiedType for ClientDiedType {
    fn what() -> &'static str {
        "client"
    }

    fn wrap_error(e: Arc<crate::Error>) -> crate::Error {
        crate::Error::ClientDied(e)
    }
}

impl DiedType for ConnDiedType {
    fn what() -> &'static str {
        "connection"
    }

    fn wrap_error(e: Arc<crate::Error>) -> crate::Error {
        crate::Error::ConnDied(e)
    }
}

#[derive(Default, Clone, Debug)]
pub(crate) struct SomethingDiedErrorHolder<D: DiedType> {
    error: Arc<Mutex<Option<Arc<error::Error>>>>,
    _marker: marker::PhantomData<D>,
}

impl<D: DiedType> SomethingDiedErrorHolder<D> {
    pub fn new() -> Self {
        Default::default()
    }

    fn raw_error(&self) -> Arc<error::Error> {
        let lock = self.error.lock().unwrap();
        (*lock)
            .clone()
            .unwrap_or_else(|| Arc::new(crate::Error::DeathReasonUnknown))
    }

    pub fn error(&self) -> error::Error {
        D::wrap_error(self.raw_error())
    }

    pub fn set_once(&self, error: error::Error) {
        let mut lock = self.error.lock().unwrap();
        if (*lock).is_none() {
            *lock = Some(Arc::new(error));
        }
    }

    pub fn wrap_future_keep_result<R, F>(
        &self,
        future: F,
    ) -> impl Future<Output = Result<R, ()>> + Send
    where
        R: Send,
        F: Future<Output = crate::Result<R>> + Send,
    {
        let holder = self.clone();
        let future = future.then(move |r| match r {
            Ok(r) => {
                holder.set_once(error::Error::ClientCompletedWithoutError);
                future::ok(r)
            }
            Err(e) => {
                warn!("{} completed with error: {:?}", D::what(), e);
                holder.set_once(e);
                future::err(())
            }
        });

        let holder = self.clone();
        let future = AssertUnwindSafe(future)
            .catch_unwind()
            .then(move |r| match r {
                Err(e) => {
                    let message = any_to_string(e);
                    warn!("{} panicked: {}", D::what(), message);
                    holder.set_once(error::Error::ClientPanicked(message));
                    future::err(())
                }
                Ok(r) => future::ready(r),
            });

        future
    }

    pub fn wrap_future<F>(&self, future: F) -> impl Future<Output = ()> + Send
    where
        F: Future<Output = crate::Result<()>> + Send,
    {
        self.wrap_future_keep_result(future).then(|r| match r {
            Ok(()) => {
                info!("{} completed without errors", D::what());
                future::ready(())
            }
            Err(()) => future::ready(()),
        })
    }
}
