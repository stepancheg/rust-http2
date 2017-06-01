use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;
use std::ptr;


/// Atomic `Box<A>` or integer 0..4
pub struct AtomicIntOrBox<A>(AtomicPtr<A>);

#[derive(Debug)]
pub enum DecodedRef<A> {
    Ptr(*mut A),
    Int(u32),
}

#[derive(Debug)]
pub enum DecodedBox<A> {
    Box(Box<A>),
    Int(u32),
}

impl<A> DecodedRef<A> {
    fn into_raw(self) -> *mut A {
        match self {
            DecodedRef::Ptr(p) => p,
            DecodedRef::Int(v) => {
                assert!(v < 4);
                v as *mut A
            }
        }
    }

    unsafe fn into_box(self) -> DecodedBox<A> {
        match self {
            DecodedRef::Ptr(p) => DecodedBox::Box(Box::from_raw(p)),
            DecodedRef::Int(i) => DecodedBox::Int(i),
        }
    }

    fn from_raw(ptr: *mut A) -> DecodedRef<A> {
        let v = ptr as usize;
        if v < 4 {
            DecodedRef::Int(v as u32)
        } else {
            DecodedRef::Ptr(ptr)
        }
    }
}

impl<A> DecodedBox<A> {
    unsafe fn into_ref(self) -> DecodedRef<A> {
        self.as_ref()
    }

    fn as_ref(&self) -> DecodedRef<A> {
        match self {
            &DecodedBox::Int(i) => DecodedRef::Int(i),
            &DecodedBox::Box(ref b) => DecodedRef::Ptr(Box::as_ref(b) as *const A as *mut A),
        }
    }

    unsafe fn into_raw(self) -> *mut A {
        self.into_ref().into_raw()
    }

    unsafe fn from_raw(ptr: *mut A) -> DecodedBox<A> {
        DecodedRef::from_raw(ptr).into_box()
    }
}

impl<A> AtomicIntOrBox<A> {
    pub fn new() -> AtomicIntOrBox<A> {
        AtomicIntOrBox(AtomicPtr::new(ptr::null_mut()))
    }

    pub fn into_inner(self) -> DecodedBox<A> {
        unsafe { self.load().into_box() }
    }

    pub fn load(&self) -> DecodedRef<A> {
        DecodedRef::from_raw(self.0.load(Ordering::SeqCst))
    }

    pub fn store(&self, value: DecodedBox<A>) {
        self.swap(value);
    }

    pub fn swap(&self, that: DecodedBox<A>) -> DecodedBox<A> {
        unsafe { DecodedBox::from_raw(self.0.swap(that.into_raw(), Ordering::SeqCst)) }
    }

    pub fn compare_exchange(&self, compare: DecodedRef<A>, exchange: DecodedBox<A>)
        -> Result<DecodedBox<A>, (DecodedRef<A>, DecodedBox<A>)>
    {
        self.0.compare_exchange(
            compare.into_raw(), exchange.as_ref().into_raw(), Ordering::SeqCst, Ordering::SeqCst)
                .map(|p| unsafe { DecodedBox::from_raw(p) })
                .map_err(|p| (DecodedRef::from_raw(p), exchange))
    }

    pub fn compare_int_exchange(&self, compare: u32, exchange: DecodedBox<A>)
        -> Result<DecodedBox<A>, (DecodedRef<A>, DecodedBox<A>)>
    {
        self.compare_exchange(DecodedRef::Int(compare), exchange)
    }

    pub fn compare_ptr_exchange(&self, exchange: DecodedBox<A>)
        -> Result<DecodedBox<A>, (DecodedRef<A>, DecodedBox<A>)>
    {
        match self.load() {
            DecodedRef::Int(i) => Err((DecodedRef::Int(i), exchange)),
            DecodedRef::Ptr(p) => self.compare_exchange(DecodedRef::Ptr(p), exchange),
        }
    }
}

#[cfg(test)]
mod test {
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
    fn test() {
        let count = Arc::new(AtomicUsize::new(0));

        let b = AtomicIntOrBox::new();

        match b.load() {
            DecodedRef::Int(0) => (),
            _ => panic!(),
        }

        b.store(DecodedBox::Box(Box::new(Canary::new(count.clone()))));

        assert_eq!(0, count.load(Ordering::SeqCst));
    }

}
