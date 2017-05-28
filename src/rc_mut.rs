use std::cell::RefCell;
pub use std::cell::RefMut;

use std::rc::Rc;

/// Convenient wrapper around `Rc` + `RefCell`
pub struct RcMut<A>(Rc<RefCell<A>>);

impl<A> Clone for RcMut<A> {
    fn clone(&self) -> Self {
        RcMut(self.0.clone())
    }
}

impl<A> RcMut<A> {
    pub fn new(a: A) -> RcMut<A> {
        RcMut(Rc::new(RefCell::new(a)))
    }

    pub fn borrow_mut(&self) -> RefMut<A> {
        self.0.borrow_mut()
    }

    pub fn with<F, R>(&self, f: F) -> R
        where F: FnOnce(&mut A) -> R
    {
        f(&mut *self.borrow_mut())
    }
}
