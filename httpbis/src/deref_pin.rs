use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;

/// Similar to `DerefMut`, but dereferences `Pin` to `Pin`.
pub trait DerefPinMut: DerefMut {
    /// Deref.
    fn deref_pin(self: Pin<&mut Self>) -> Pin<&mut Self::Target>;
}

impl<T: ?Sized + Unpin> DerefPinMut for &mut T {
    fn deref_pin(self: Pin<&mut Self>) -> Pin<&mut T> {
        Pin::new(&mut *self.get_mut())
    }
}

impl<T: ?Sized + Unpin> DerefPinMut for Box<T> {
    fn deref_pin(self: Pin<&mut Self>) -> Pin<&mut T> {
        Pin::new(&mut *self.get_mut())
    }
}

impl<P> DerefPinMut for Pin<P>
where
    P: DerefMut + Unpin,
    <P as Deref>::Target: Unpin,
{
    fn deref_pin(self: Pin<&mut Self>) -> Pin<&mut P::Target> {
        self.get_mut().as_mut()
    }
}
