use std::fmt;

/// Errors returned by generators and IO.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The job was cancelled; no partial output is returned.
    Cancelled,
    /// The caller's parameters are outside the supported range.
    InvalidParams(String),
    /// An internal invariant failed. This is a bug.
    Internal(String),
    /// File or decode error (IO helpers only; generators never do IO).
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => write!(f, "cancelled"),
            Error::InvalidParams(m) => write!(f, "invalid parameters: {m}"),
            Error::Internal(m) => write!(f, "internal error: {m}"),
            Error::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// A non-fatal note for the UI (for example "relief below printable detail").
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub message: String,
}
