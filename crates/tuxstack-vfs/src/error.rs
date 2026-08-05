use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    NotDirectory,
    IsDirectory,
    ReadOnly,
    PermissionDenied,
    InvalidInput(&'static str),
    NameTooLong,
    PathTooLong,
    InvalidEncoding,
    TooManyHandles,
    BadHandle,
    Stale,
    SymlinkLoop,
    SymlinkEscape,
    SpecialFile,
    TimedOut,
    Unsupported(String),
    Unavailable(String),
    Io(String),
}

impl VfsError {
    pub fn errno(&self) -> i32 {
        match self {
            Self::NotFound => libc::ENOENT,
            Self::NotDirectory => libc::ENOTDIR,
            Self::IsDirectory => libc::EISDIR,
            Self::ReadOnly => libc::EROFS,
            Self::PermissionDenied => libc::EACCES,
            Self::InvalidInput(_) | Self::InvalidEncoding => libc::EINVAL,
            Self::NameTooLong | Self::PathTooLong => libc::ENAMETOOLONG,
            Self::TooManyHandles => libc::EMFILE,
            Self::BadHandle => libc::EBADF,
            Self::Stale => libc::ESTALE,
            Self::SymlinkLoop => libc::ELOOP,
            Self::SymlinkEscape | Self::SpecialFile => libc::ENXIO,
            Self::TimedOut => libc::ETIMEDOUT,
            Self::Unsupported(_) => libc::EOPNOTSUPP,
            Self::Unavailable(_) => libc::ENOTCONN,
            Self::Io(_) => libc::EIO,
        }
    }
}

impl fmt::Display for VfsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid input: {message}"),
            Self::Unsupported(message) => write!(formatter, "operation unsupported: {message}"),
            Self::Unavailable(message) => write!(formatter, "provider unavailable: {message}"),
            Self::Io(message) => write!(formatter, "I/O error: {message}"),
            other => write!(formatter, "{other:?}"),
        }
    }
}

impl std::error::Error for VfsError {}

impl From<std::io::Error> for VfsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
