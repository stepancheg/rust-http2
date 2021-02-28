use std::any;
use std::fmt;
use std::fmt::Formatter;

pub(crate) struct DebugUndebug<A>(pub A);

impl<A> fmt::Debug for DebugUndebug<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let _ = &self.0;
        write!(f, "{}(...)", any::type_name::<A>())
    }
}
