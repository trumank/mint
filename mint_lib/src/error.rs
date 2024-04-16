use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(display("mint encountered an error: {msg}"))]
pub struct GenericError {
    pub msg: String,
}

pub trait ResultExt<T, E> {
    fn generic(self, msg: String) -> Result<T, GenericError>;
    fn with_generic<F>(self, f: F) -> Result<T, GenericError>
    where
        F: FnOnce(E) -> String;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn generic(self, msg: String) -> Result<T, GenericError> {
        match self {
            Ok(ok) => Ok(ok),
            Err(_) => Err(GenericError { msg }),
        }
    }
    fn with_generic<F>(self, f: F) -> Result<T, GenericError>
    where
        F: FnOnce(E) -> String,
    {
        match self {
            Ok(ok) => Ok(ok),
            Err(e) => Err(GenericError { msg: f(e) }),
        }
    }
}
