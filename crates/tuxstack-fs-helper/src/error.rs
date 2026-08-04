//! Helper-side error type built on the stable protocol error codes.

use std::io;

use tuxstack_fs_protocol::HelperMessage;

pub use tuxstack_fs_protocol::HelperErrorCode;

#[derive(Debug)]
pub struct HelperError {
    pub code: HelperErrorCode,
    pub message: String,
}

impl HelperError {
    pub fn new(code: HelperErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn from_io(path: &std::path::Path, error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::new(
                HelperErrorCode::NotFound,
                format!("no such file or directory: {}", path.display()),
            ),
            io::ErrorKind::PermissionDenied => Self::new(
                HelperErrorCode::PermissionDenied,
                format!("permission denied: {}", path.display()),
            ),
            io::ErrorKind::IsADirectory => Self::new(
                HelperErrorCode::IsDirectory,
                format!("is a directory: {}", path.display()),
            ),
            io::ErrorKind::NotADirectory => Self::new(
                HelperErrorCode::NotDirectory,
                format!("not a directory: {}", path.display()),
            ),
            _ => Self::new(
                HelperErrorCode::Io,
                format!("I/O error on {}: {error}", path.display()),
            ),
        }
    }

    pub fn into_message(self) -> HelperMessage {
        HelperMessage::Error {
            code: self.code,
            message: self.message,
        }
    }
}

pub type Result<T> = std::result::Result<T, HelperError>;
