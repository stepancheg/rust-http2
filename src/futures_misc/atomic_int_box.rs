use std::fmt;

use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;
use std::ptr;
use std::mem;


// Two bit integer
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum U2 {
    V0 = 0,
    V1 = 1,
    V2 = 2,
    V3 = 3,
}

impl fmt::Debug for U2 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.to_u32(), f)
    }
}

impl U2 {
    pub fn to_u32(&self) -> u32 {
        *self as u32
    }

    pub fn from_u32(v: u32) -> U2 {
        match v {
            0 => U2::V0,
            1 => U2::V1,
            2 => U2::V2,
            3 => U2::V3,
            n => panic!("cannot convert to U2: {}", n),
        }
    }

    pub fn from_usize(v: usize) -> U2 {
        match v {
            0 => U2::V0,
            1 => U2::V1,
            2 => U2::V2,
            3 => U2::V3,
            n => panic!("cannot convert to U2: {}", n),
        }
    }
}


/// Atomic `Box<A>` or integer 0..4
pub struct AtomicU2OrBox<A>(AtomicPtr<A>);

#[derive(Debug)]
pub enum DecodedRef<A> {
    Ptr(*mut A),
    U2(U2),
}

#[derive(Debug)]
pub enum DecodedBox<A> {
    Box(Box<A>),
    U2(U2),
}

impl<A> DecodedRef<A> {
    fn into_raw(self) -> *mut A {
        match self {
            DecodedRef::Ptr(p) => p,
            DecodedRef::U2(v) => {
                v.to_u32() as *mut A
            }
        }
    }

    unsafe fn into_box(self) -> DecodedBox<A> {
        match self {
            DecodedRef::Ptr(p) => DecodedBox::Box(Box::from_raw(p)),
            DecodedRef::U2(i) => DecodedBox::U2(i),
        }
    }

    fn from_raw(ptr: *mut A) -> DecodedRef<A> {
        let v = ptr as usize;
        if v < 4 {
            DecodedRef::U2(U2::from_usize(v))
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
            &DecodedBox::U2(i) => DecodedRef::U2(i),
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

impl<A> AtomicU2OrBox<A> {
    pub fn new() -> AtomicU2OrBox<A> {
        AtomicU2OrBox(AtomicPtr::new(ptr::null_mut()))
    }

    pub fn from(b: DecodedBox<A>) -> AtomicU2OrBox<A> {
        AtomicU2OrBox(AtomicPtr::new(unsafe { b.into_raw() }))
    }

    pub fn from_u2(v: U2) -> AtomicU2OrBox<A> {
        AtomicU2OrBox::from(DecodedBox::U2(v))
    }

    pub fn from_u32(v: u32) -> AtomicU2OrBox<A> {
        AtomicU2OrBox::from_u2(U2::from_u32(v))
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
        match self.0.compare_exchange(
            compare.into_raw(), exchange.as_ref().into_raw(), Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(p) => unsafe { mem::forget(exchange); Ok(DecodedBox::from_raw(p)) },
            Err(p) => Err((DecodedRef::from_raw(p), exchange)),
        }
    }

    pub fn compare_int_exchange(&self, compare: U2, exchange: DecodedBox<A>)
        -> Result<DecodedBox<A>, (DecodedRef<A>, DecodedBox<A>)>
    {
        self.compare_exchange(DecodedRef::U2(compare), exchange)
    }

    pub fn compare_ptr_exchange(&self, exchange: DecodedBox<A>)
        -> Result<DecodedBox<A>, (DecodedRef<A>, DecodedBox<A>)>
    {
        match self.load() {
            DecodedRef::U2(i) => Err((DecodedRef::U2(i), exchange)),
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

        let b = AtomicU2OrBox::new();

        match b.load() {
            DecodedRef::U2(U2::V0) => (),
            _ => panic!(),
        }

        b.store(DecodedBox::Box(Box::new(Canary::new(count.clone()))));

        assert_eq!(0, count.load(Ordering::SeqCst));
    }

}
