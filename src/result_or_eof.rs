#[derive(Debug)]
pub enum ResultOrEof<T, E> {
    Item(T),
    Error(E),
    Eof,
}

impl<T, E> ResultOrEof<T, E> {
    pub fn _and_then<U, F: FnOnce(T) -> Result<U, E>>(self, op: F) -> ResultOrEof<U, E> {
        match self {
            ResultOrEof::Item(t) => match op(t) {
                Ok(r) => ResultOrEof::Item(r),
                Err(e) => ResultOrEof::Error(e),
            },
            ResultOrEof::Error(e) => ResultOrEof::Error(e),
            ResultOrEof::Eof => ResultOrEof::Eof,
        }
    }
}

impl<T, E> From<Result<T, E>> for ResultOrEof<T, E> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(r) => ResultOrEof::Item(r),
            Err(e) => ResultOrEof::Error(e),
        }
    }
}
