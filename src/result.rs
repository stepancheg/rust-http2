use std;

use error;

/// A convenience `Result` type that has the `HttpError` type as the error
/// type and a generic Ok result type.
pub type Result<T> = std::result::Result<T, error::Error>;
