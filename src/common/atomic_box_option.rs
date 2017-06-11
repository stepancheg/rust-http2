#![allow(dead_code)]

use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;
use std::ptr;

/// Atomic `Box<Option<T>>`
pub struct AtomicBoxOption<T> {
    ptr: AtomicPtr<T>,
}

impl<T> Drop for AtomicBoxOption<T> {
    fn drop(&mut self) {
        let ptr = self.load_raw(Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe { Box::from_raw(ptr); }
        }
    }
}


unsafe fn into_raw<T>(o: Option<Box<T>>) -> *mut T {
    match o {
        None => ptr::null_mut(),
        Some(b) => Box::into_raw(b),
    }
}

unsafe fn from_raw<T>(p: *mut T) -> Option<Box<T>> {
    if p.is_null() {
        None
    } else {
        Some(Box::from_raw(p))
    }
}


impl<T> AtomicBoxOption<T> {
    pub fn new() -> AtomicBoxOption<T> {
        AtomicBoxOption {
            ptr: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub fn load_raw(&self, ordering: Ordering) -> *mut T {
        self.ptr.load(ordering)
    }

    pub fn load_is_some(&self, ordering: Ordering) -> bool {
        !self.load_raw(ordering).is_null()
    }

    pub fn swap(&self, value: Option<Box<T>>, ordering: Ordering) -> Option<Box<T>> {
        let ptr = match value {
            None => ptr::null_mut(),
            Some(b) => Box::into_raw(b),
        };

        let prev = self.ptr.swap(ptr, ordering);
        unsafe { from_raw(prev) }
    }

    pub fn swap_null(&self, ordering: Ordering) -> Option<Box<T>> {
        self.swap(None, ordering)
    }

    pub fn store_null(&self, ordering: Ordering) {
        self.swap_null(ordering);
    }

    pub fn store_box(&self, value: Box<T>, ordering: Ordering) {
        self.swap(Some(value), ordering);
    }

    pub fn compare_exchange(&self,
        compare: *mut T, exchange: Option<Box<T>>,
        success: Ordering, failure: Ordering)
        -> Result<Option<Box<T>>, (*mut T, Option<Box<T>>)>
    {
        let exchange = unsafe { into_raw(exchange) };

        match self.ptr.compare_exchange(compare, exchange, success, failure) {
            Ok(p) => unsafe { Ok(from_raw(p)) },
            Err(p) => Err((p, unsafe { from_raw(exchange) })),
        }
    }
}

#[cfg(test)]
mod test {

    use std::ptr;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::*;

    #[derive(Debug)]
    struct Canary {
        count: Arc<AtomicUsize>,
    }

    impl Canary {
        fn new(count: Arc<AtomicUsize>) -> Canary {
            count.fetch_add(1, Ordering::Relaxed);
            Canary {
                count: count,
            }
        }
    }

    impl Drop for Canary {
        fn drop(&mut self) {
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn swap() {
        let count = Arc::new(AtomicUsize::new(0));

        let b = AtomicBoxOption::new();

        assert!(b.swap(None, Ordering::SeqCst).is_none());
        assert!(b.swap(Some(Box::new(Canary::new(count.clone()))), Ordering::SeqCst).is_none());
        assert!(b.swap(Some(Box::new(Canary::new(count.clone()))), Ordering::SeqCst).is_some());

        drop(b);

        assert_eq!(0, count.load(Ordering::SeqCst));
    }

    #[test]
    fn compare_exchange() {
        let count = Arc::new(AtomicUsize::new(0));

        let b = AtomicBoxOption::new();

        assert!(b.compare_exchange(
            ptr::null_mut(), None,
            Ordering::SeqCst, Ordering::SeqCst)
                .is_ok());
        assert!(b.compare_exchange(
            ptr::null_mut(), Some(Box::new(Canary::new(count.clone()))),
            Ordering::SeqCst, Ordering::SeqCst)
                .is_ok());
        assert!(b.compare_exchange(
            ptr::null_mut(), Some(Box::new(Canary::new(count.clone()))),
            Ordering::SeqCst, Ordering::SeqCst)
                .is_err());

        drop(b);

        assert_eq!(0, count.load(Ordering::SeqCst));
    }

}
