use std::{fmt, io};

use crate::frame;
use tokio::sync::mpsc::error::SendError;

#[derive(Debug)]
pub(crate) enum Error {
    NetworkIoError(String),
    InternalChannelError(String),
    SerdeError(String),
    FramingError(String),
    CloseCallbackError,
    Interrupted,
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::NetworkIoError(err.to_string())
    }
}
impl From<amqp_serde::Error> for Error {
    fn from(err: amqp_serde::Error) -> Self {
        Error::SerdeError(err.to_string())
    }
}
impl From<frame::Error> for Error {
    fn from(err: frame::Error) -> Self {
        Error::FramingError(err.to_string())
    }
}
impl<T> From<SendError<T>> for Error {
    fn from(err: SendError<T>) -> Self {
        Error::InternalChannelError(err.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NetworkIoError(msg) => write!(f, "network io error: {}", msg),
            Error::InternalChannelError(msg) => write!(f, "internal communication error: {}", msg),
            Error::SerdeError(msg) => write!(f, "serde error: {}", msg),
            Error::FramingError(msg) => write!(f, "framing error: {}", msg),
            Error::CloseCallbackError => f.write_str("peer shutdown"),
            Error::Interrupted => f.write_str("connection interrupted"),
        }
    }
}

impl std::error::Error for Error {}
