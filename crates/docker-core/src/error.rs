//! Error types for the tuxstack Docker core.

use std::path::PathBuf;

/// Unified error type for all Docker operations.
///
/// GUI and CLI map these variants to user-facing messages. The full
/// underlying error chain is available via `source()` and is only shown
/// in debug logs, never on screen.
#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker socket was not found at {0}")]
    SocketNotFound(PathBuf),

    #[error("Permission denied while accessing Docker")]
    PermissionDenied,

    #[error("Docker Engine is unavailable")]
    EngineUnavailable,

    #[error("Docker connection timed out")]
    ConnectionTimeout,

    #[error("Docker operation timed out")]
    OperationTimeout,

    #[error("Container was not found: {0}")]
    ContainerNotFound(String),

    #[error("Image was not found: {0}")]
    ImageNotFound(String),

    #[error("Network was not found: {0}")]
    NetworkNotFound(String),

    #[error("Volume was not found: {0}")]
    VolumeNotFound(String),

    #[error("Docker operation conflicts with current state: {0}")]
    Conflict(String),

    #[error("Invalid Docker response: {0}")]
    InvalidResponse(String),

    #[error("Docker API error: {0}")]
    Api(String),

    #[error("Unsupported Docker connection method: {0}")]
    UnsupportedConnection(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Classify a bollard connection error into a precise [`DockerError`].
pub(crate) fn classify_connect_error(
    err: &bollard::errors::Error,
    socket: Option<&PathBuf>,
) -> DockerError {
    match err {
        bollard::errors::Error::DockerResponseServerError {
            status_code: 401 | 403,
            ..
        } => DockerError::PermissionDenied,
        bollard::errors::Error::SocketNotFoundError(path) => {
            DockerError::SocketNotFound(PathBuf::from(path))
        }
        bollard::errors::Error::RequestTimeoutError => DockerError::ConnectionTimeout,
        _ => {
            let text = err.to_string().to_lowercase();
            if text.contains("permission denied") || text.contains("operation not permitted") {
                DockerError::PermissionDenied
            } else if text.contains("no such file") || text.contains("no such file or directory") {
                DockerError::SocketNotFound(
                    socket
                        .cloned()
                        .unwrap_or_else(|| PathBuf::from("/var/run/docker.sock")),
                )
            } else if text.contains("connection refused")
                || text.contains("connect refused")
                || text.contains("no route to host")
            {
                DockerError::EngineUnavailable
            } else if text.contains("timed out") || text.contains("timeout") {
                DockerError::ConnectionTimeout
            } else if text.contains("dns error") || text.contains("invalid uri") {
                DockerError::UnsupportedConnection(err.to_string())
            } else {
                DockerError::Api(err.to_string())
            }
        }
    }
}

/// Classify a bollard API error and HTTP status code into a [`DockerError`].
pub(crate) fn classify_api_error(err: &bollard::errors::Error, resource: &str) -> DockerError {
    match err {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message,
        } => match *status_code {
            404 => not_found(resource, message),
            409 => DockerError::Conflict(message.clone()),
            401 | 403 => DockerError::PermissionDenied,
            408 => DockerError::OperationTimeout,
            _ => DockerError::Api(format!("Docker API error ({status_code}): {message}")),
        },
        bollard::errors::Error::RequestTimeoutError => DockerError::OperationTimeout,
        _ => {
            let text = err.to_string().to_lowercase();
            if text.contains("timed out") || text.contains("timeout") {
                DockerError::OperationTimeout
            } else if text.contains("permission denied") {
                DockerError::PermissionDenied
            } else {
                DockerError::Api(err.to_string())
            }
        }
    }
}

fn not_found(resource: &str, message: &str) -> DockerError {
    // `resource` is a lowercase plural kind: "container", "image", ...
    match resource {
        "container" => DockerError::ContainerNotFound(message.to_string()),
        "image" => DockerError::ImageNotFound(message.to_string()),
        "network" => DockerError::NetworkNotFound(message.to_string()),
        "volume" => DockerError::VolumeNotFound(message.to_string()),
        _ => DockerError::Api(format!("Not found: {message}")),
    }
}
