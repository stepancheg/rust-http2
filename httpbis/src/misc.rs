use std::any::Any;
use std::fmt;

#[allow(dead_code)]
pub struct BsDebug<'a>(pub &'a [u8]);

fn fmt_b(b: u8, f: &mut fmt::Formatter) -> fmt::Result {
    // ASCII printable
    if b >= 0x20 && b < 0x7f {
        write!(f, "{}", b as char)
    } else {
        write!(f, "\\x{:02x}", b)
    }
}

impl<'a> fmt::Debug for BsDebug<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.0.len() > 20 && self.0.iter().all(|&b| b == self.0[0]) {
            write!(fmt, "{}*b\"", self.0.len())?;
            fmt_b(self.0[0], fmt)?;
            write!(fmt, "\"")?;
            return Ok(());
        }

        write!(fmt, "b\"")?;
        let u8a: &[u8] = self.0;
        for &c in u8a {
            fmt_b(c, fmt)?;
        }
        write!(fmt, "\"")?;
        Ok(())
    }
}

pub fn any_to_string(any: Box<dyn Any + Send + 'static>) -> String {
    if any.is::<String>() {
        *any.downcast::<String>().unwrap()
    } else if any.is::<&str>() {
        (*any.downcast::<&str>().unwrap()).to_owned()
    } else {
        "unknown any".to_owned()
    }
}
