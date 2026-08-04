//! Error types for the tuxstack Docker core.

use std::path::PathBuf;

/// Unified error type for all Docker operations.
///
/// The GUI maps these variants to user-facing messages. The full
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

    #[error("Invalid image reference: {0}")]
    InvalidImageReference(String),

    #[error("Registry authentication failed")]
    RegistryAuthenticationFailed,

    #[error("Registry is unavailable: {0}")]
    RegistryUnavailable(String),

    #[error("Image pull failed: {0}")]
    PullFailed(String),

    #[error("Image export failed: {0}")]
    ExportFailed(String),

    #[error("Operation was cancelled")]
    OperationCancelled,

    #[error("Destination permission denied: {0}")]
    DestinationPermissionDenied(PathBuf),

    #[error("Not enough space at export destination: {0}")]
    DiskFull(PathBuf),

    #[error("Network was not found: {0}")]
    NetworkNotFound(String),

    #[error("Docker-managed network cannot be removed: {0}")]
    NetworkProtected(String),

    #[error("Network is currently in use: {0}")]
    NetworkInUse(String),

    #[error("Invalid network configuration: {0}")]
    InvalidNetworkConfig(String),

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

/// Collect the full error chain text (self + sources) for classification.
fn chain_text(err: &bollard::errors::Error) -> String {
    use std::error::Error as _;
    let mut parts = vec![err.to_string()];
    let mut next: Option<&dyn std::error::Error> = err.source();
    while let Some(e) = next {
        parts.push(e.to_string());
        next = e.source();
    }
    parts.join(" | ").to_lowercase()
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
            let text = chain_text(err);
            if text.contains("permission denied") || text.contains("os error 13") {
                DockerError::PermissionDenied
            } else if text.contains("no such file") || text.contains("os error 2") {
                DockerError::SocketNotFound(
                    socket
                        .cloned()
                        .unwrap_or_else(|| PathBuf::from("/var/run/docker.sock")),
                )
            } else if text.contains("connection refused")
                || text.contains("no route to host")
                || text.contains("os error 111")
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
            let text = chain_text(err);
            if text.contains("timed out") || text.contains("timeout") {
                DockerError::OperationTimeout
            } else if text.contains("permission denied") || text.contains("os error 13") {
                DockerError::PermissionDenied
            } else if text.contains("connection refused") || text.contains("os error 111") {
                DockerError::EngineUnavailable
            } else {
                DockerError::Api(err.to_string())
            }
        }
    }
}

/// Apply network-only API classifications without changing conflict or
/// validation behavior for images and other resources.
pub(crate) fn classify_network_api_error(
    err: &bollard::errors::Error,
    operation: &str,
) -> DockerError {
    if let bollard::errors::Error::DockerResponseServerError {
        status_code,
        message,
    } = err
    {
        let lower = message.to_ascii_lowercase();
        let protected = operation == "remove"
            && (lower.contains("pre-defined network")
                || lower.contains("predefined network")
                || lower.contains("built-in network")
                || lower.contains("builtin network"));
        if protected {
            return DockerError::NetworkProtected(message.clone());
        }
        let in_use = lower.contains("active endpoint")
            || lower.contains("network is in use")
            || lower.contains("network in use");
        if in_use {
            return DockerError::NetworkInUse(message.clone());
        }
        if matches!(*status_code, 401 | 403) {
            return DockerError::PermissionDenied;
        }

        let config_message = lower.contains("invalid")
            || lower.contains("pool overlaps")
            || lower.contains("non-overlapping")
            || lower.contains("address pool")
            || lower.contains("subnet")
            || lower.contains("gateway")
            || lower.contains("driver")
            || lower.contains("plugin")
            || lower.contains("already exists");
        if *status_code == 400 || (operation == "create" && (*status_code == 409 || config_message))
        {
            return DockerError::InvalidNetworkConfig(message.clone());
        }
    }

    classify_api_error(err, "network")
}

fn not_found(resource: &str, message: &str) -> DockerError {
    // `resource` is a lowercase singular kind: "container", "image", ...
    match resource {
        "container" => DockerError::ContainerNotFound(message.to_string()),
        "image" => DockerError::ImageNotFound(message.to_string()),
        "network" => DockerError::NetworkNotFound(message.to_string()),
        "volume" => DockerError::VolumeNotFound(message.to_string()),
        _ => DockerError::Api(format!("Not found: {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status_code: u16, message: &str) -> bollard::errors::Error {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message: message.to_string(),
        }
    }

    #[test]
    fn image_404_is_typed_not_found() {
        assert!(matches!(
            classify_api_error(&response(404, "No such image"), "image"),
            DockerError::ImageNotFound(_)
        ));
    }

    #[test]
    fn image_409_is_typed_conflict() {
        assert!(matches!(
            classify_api_error(&response(409, "image is being used"), "image"),
            DockerError::Conflict(message) if message.contains("being used")
        ));
    }

    #[test]
    fn network_active_endpoints_are_typed_without_changing_image_conflicts() {
        let error = response(403, "network test has active endpoints");
        assert!(matches!(
            classify_network_api_error(&error, "remove"),
            DockerError::NetworkInUse(message) if message.contains("active endpoints")
        ));
        assert!(matches!(
            classify_api_error(&response(409, "image is being used"), "image"),
            DockerError::Conflict(_)
        ));
    }

    #[test]
    fn invalid_network_create_config_is_typed() {
        for error in [
            response(400, "invalid subnet 172.30.0.0/99"),
            response(404, "network driver plugin not found"),
            response(409, "network with name test already exists"),
        ] {
            assert!(matches!(
                classify_network_api_error(&error, "create"),
                DockerError::InvalidNetworkConfig(_)
            ));
        }
    }

    #[test]
    fn network_not_found_protected_and_permission_remain_precise() {
        assert!(matches!(
            classify_network_api_error(&response(404, "No such network"), "inspect"),
            DockerError::NetworkNotFound(_)
        ));
        assert!(matches!(
            classify_network_api_error(
                &response(403, "bridge is a pre-defined network and cannot be removed"),
                "remove"
            ),
            DockerError::NetworkProtected(_)
        ));
        assert!(matches!(
            classify_network_api_error(&response(403, "authorization denied"), "remove"),
            DockerError::PermissionDenied
        ));
    }

    #[test]
    fn authorization_failures_are_permission_denied() {
        for status in [401, 403] {
            assert!(matches!(
                classify_api_error(&response(status, "denied"), "image"),
                DockerError::PermissionDenied
            ));
        }
    }

    #[test]
    fn timeout_status_and_bollard_timeout_are_typed() {
        assert!(matches!(
            classify_api_error(&response(408, "timeout"), "image"),
            DockerError::OperationTimeout
        ));
        assert!(matches!(
            classify_api_error(&bollard::errors::Error::RequestTimeoutError, "image"),
            DockerError::OperationTimeout
        ));
    }
}
