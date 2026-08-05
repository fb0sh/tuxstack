//! Error types for the unified filesystem browsing service.

use std::fmt;

/// Stable error codes for the filesystem browsing service. These map to the
/// protocol's `HelperErrorCode` and to stable IPC codes for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemError {
    /// The Docker daemon is not reachable.
    DockerUnavailable,
    /// The image or volume could not be found.
    SourceNotFound,
    /// The image could not be found.
    ImageNotFound(String),
    /// The volume could not be found.
    VolumeNotFound(String),
    /// The target platform is not supported (e.g. Windows image on Linux).
    UnsupportedPlatform(String),
    /// The static helper binary was not compiled for this architecture.
    HelperBinaryUnavailable(String),
    /// The helper image could not be loaded into the daemon.
    HelperImageLoadFailed(String),
    /// The preview container could not be created.
    HelperContainerCreateFailed(String),
    /// The preview container could not be started.
    HelperContainerStartFailed(String),
    /// The hello handshake failed (protocol mismatch or helper crash).
    HelperHandshakeFailed(String),
    /// The helper protocol version does not match the client's.
    HelperProtocolMismatch { expected: u32, got: u32 },
    /// A JSON Lines parse error from the helper.
    HelperProtocolError(String),
    /// The path token was malformed or undecodable.
    InvalidPathToken(String),
    /// The path escaped the browse root.
    PathEscapeRejected(String),
    /// The path was not found.
    PathNotFound(String),
    /// The path is not a directory.
    NotDirectory(String),
    /// The path is a directory (not a file).
    IsDirectory(String),
    /// Access was denied.
    PermissionDenied(String),
    /// The file type is unsupported for this operation.
    UnsupportedFileType(String),
    /// The response exceeded size limits.
    ResponseTooLarge(String),
    /// Too many directory entries.
    DirectoryEntryLimitExceeded,
    /// An exec operation failed.
    ExecFailed(String),
    /// The operation timed out.
    Timeout,
    /// The operation was cancelled.
    Cancelled,
    /// The session was invalidated (container removed or crashed).
    SessionInvalidated,
    /// The session container is no longer running.
    SessionClosed,
    /// A session creation or validation failed.
    SessionFailed(String),
    /// An image was not found.
    ImageNotFoundVariant(String),
    /// An operation timed out (alias for Timeout, used by the old API).
    OperationTimeout,
    /// A generic internal error.
    Internal(String),
}

impl fmt::Display for FilesystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DockerUnavailable => write!(f, "Docker Engine is unavailable"),
            Self::SourceNotFound => write!(f, "source not found"),
            Self::ImageNotFound(id) => write!(f, "image not found: {id}"),
            Self::VolumeNotFound(name) => write!(f, "volume not found: {name}"),
            Self::UnsupportedPlatform(msg) => write!(f, "unsupported platform: {msg}"),
            Self::HelperBinaryUnavailable(msg) => write!(f, "helper binary unavailable: {msg}"),
            Self::HelperImageLoadFailed(msg) => write!(f, "helper image load failed: {msg}"),
            Self::HelperContainerCreateFailed(msg) => write!(f, "container create failed: {msg}"),
            Self::HelperContainerStartFailed(msg) => write!(f, "container start failed: {msg}"),
            Self::HelperHandshakeFailed(msg) => write!(f, "helper handshake failed: {msg}"),
            Self::HelperProtocolMismatch { expected, got } => {
                write!(
                    f,
                    "protocol version mismatch: expected {expected}, got {got}"
                )
            }
            Self::HelperProtocolError(msg) => write!(f, "helper protocol error: {msg}"),
            Self::InvalidPathToken(msg) => write!(f, "invalid path token: {msg}"),
            Self::PathEscapeRejected(msg) => write!(f, "path escape rejected: {msg}"),
            Self::PathNotFound(path) => write!(f, "not found: {path}"),
            Self::NotDirectory(path) => write!(f, "not a directory: {path}"),
            Self::IsDirectory(path) => write!(f, "is a directory: {path}"),
            Self::PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::UnsupportedFileType(msg) => write!(f, "unsupported file type: {msg}"),
            Self::ResponseTooLarge(msg) => write!(f, "response too large: {msg}"),
            Self::DirectoryEntryLimitExceeded => write!(f, "directory entry limit exceeded"),
            Self::ExecFailed(msg) => write!(f, "exec failed: {msg}"),
            Self::Timeout => write!(f, "operation timed out"),
            Self::Cancelled => write!(f, "operation was cancelled"),
            Self::SessionInvalidated => write!(f, "session invalidated"),
            Self::SessionClosed => write!(f, "session closed"),
            Self::SessionFailed(msg) => write!(f, "session failed: {msg}"),
            Self::ImageNotFoundVariant(msg) => write!(f, "image not found: {msg}"),
            Self::OperationTimeout => write!(f, "operation timed out"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for FilesystemError {}

impl From<bollard::errors::Error> for FilesystemError {
    fn from(error: bollard::errors::Error) -> Self {
        Self::ExecFailed(error.to_string())
    }
}

impl FilesystemError {
    /// Stable error code string for IPC / UI mapping.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DockerUnavailable => "docker_unavailable",
            Self::SourceNotFound => "source_not_found",
            Self::ImageNotFound(_) | Self::ImageNotFoundVariant(_) => "image_not_found",
            Self::VolumeNotFound(_) => "volume_not_found",
            Self::UnsupportedPlatform(_) => "unsupported_platform",
            Self::HelperBinaryUnavailable(_) => "helper_binary_unavailable",
            Self::HelperImageLoadFailed(_) => "helper_image_load_failed",
            Self::HelperContainerCreateFailed(_) => "helper_container_create_failed",
            Self::HelperContainerStartFailed(_) => "helper_container_start_failed",
            Self::HelperHandshakeFailed(_) => "helper_handshake_failed",
            Self::HelperProtocolMismatch { .. } => "helper_protocol_mismatch",
            Self::HelperProtocolError(_) => "helper_protocol_error",
            Self::InvalidPathToken(_) => "invalid_path_token",
            Self::PathEscapeRejected(_) => "path_escape_rejected",
            Self::PathNotFound(_) => "not_found",
            Self::NotDirectory(_) => "not_directory",
            Self::IsDirectory(_) => "is_directory",
            Self::PermissionDenied(_) => "permission_denied",
            Self::UnsupportedFileType(_) => "unsupported_file_type",
            Self::ResponseTooLarge(_) => "response_too_large",
            Self::DirectoryEntryLimitExceeded => "directory_entry_limit_exceeded",
            Self::ExecFailed(_) => "exec_failed",
            Self::Timeout | Self::OperationTimeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::SessionInvalidated | Self::SessionClosed => "session_invalidated",
            Self::SessionFailed(_) => "session_failed",
            Self::Internal(_) => "internal",
        }
    }

    /// Whether the error should cause the session to be invalidated.
    pub fn invalidates_session(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::OperationTimeout
                | Self::SessionInvalidated
                | Self::SessionClosed
                | Self::SessionFailed(_)
                | Self::ExecFailed(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(FilesystemError::Timeout.code(), "timeout");
        assert_eq!(FilesystemError::SessionClosed.code(), "session_invalidated");
        assert_eq!(
            FilesystemError::ImageNotFound("x".into()).code(),
            "image_not_found"
        );
    }

    #[test]
    fn display_is_human_readable() {
        let error = FilesystemError::PermissionDenied("/etc/shadow".into());
        assert!(error.to_string().contains("permission denied"));
    }
}
