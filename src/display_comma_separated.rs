use std::fmt;

/// Display `[]` comma separated
pub(crate) struct DisplayCommaSeparated<'a, A: fmt::Display>(pub &'a [A]);

impl<'a, A: fmt::Display> fmt::Display for DisplayCommaSeparated<'a, A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, item) in self.0.into_iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", item)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_display_comma_separated() {
        assert_eq!("", format!("{}", DisplayCommaSeparated(&[0; 0])));
        assert_eq!("1", format!("{}", DisplayCommaSeparated(&[1])));
        assert_eq!("2, 3", format!("{}", DisplayCommaSeparated(&[2, 3])));
        assert_eq!("2, 3, 4", format!("{}", DisplayCommaSeparated(&[2, 3, 4])));
    }
}
