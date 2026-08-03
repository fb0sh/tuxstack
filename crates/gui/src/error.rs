//! GUI-level error type.

use tuxstack_docker_core::DockerError;

/// Errors surfaced in the GUI. The GUI only ever shows the safe
/// user-facing text; full details go to debug logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// Docker is unreachable (socket missing / engine down).
    DockerUnavailable(String),
    /// The user lacks permission to access Docker.
    PermissionDenied,
    /// A Docker operation failed with a specific message.
    Docker(String),
    /// Local configuration could not be loaded.
    #[allow(dead_code)] // reserved; config errors currently fall back to defaults
    Config(String),
}

impl AppError {
    /// Short, safe, user-facing message.
    pub fn user_message(&self) -> String {
        match self {
            AppError::DockerUnavailable(msg) => format!(
                "Docker Engine is not available: {msg}\nCheck that the Docker daemon is running \
                 and that this user can access the Docker socket."
            ),
            AppError::PermissionDenied => "Permission denied while accessing Docker.\n\
                 Add your user to the docker group (requires logout) or grant access to the \
                 Docker socket. TuxStack will not run sudo for you."
                .to_string(),
            AppError::Docker(msg) => msg.clone(),
            AppError::Config(msg) => format!("Configuration error: {msg}"),
        }
    }

    /// Machine-readable kind used by the UI state machine.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::DockerUnavailable(_) => "docker_unavailable",
            AppError::PermissionDenied => "permission_denied",
            AppError::Docker(_) => "docker",
            AppError::Config(_) => "config",
        }
    }
}

impl From<&DockerError> for AppError {
    fn from(err: &DockerError) -> Self {
        match err {
            DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => {
                AppError::DockerUnavailable(err.to_string())
            }
            DockerError::PermissionDenied => AppError::PermissionDenied,
            other => AppError::Docker(other.to_string()),
        }
    }
}

impl From<DockerError> for AppError {
    fn from(err: DockerError) -> Self {
        AppError::from(&err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_not_found_maps_to_unavailable() {
        let err = AppError::from(&DockerError::SocketNotFound("/var/run/docker.sock".into()));
        assert_eq!(err.kind(), "docker_unavailable");
        assert!(
            err.user_message()
                .contains("Docker Engine is not available")
        );
    }

    #[test]
    fn permission_maps_to_permission() {
        let err = AppError::from(&DockerError::PermissionDenied);
        assert_eq!(err.kind(), "permission_denied");
        assert!(err.user_message().contains("docker group"));
    }

    #[test]
    fn api_error_keeps_message() {
        let err = AppError::from(&DockerError::Api("boom".into()));
        assert_eq!(err.user_message(), "Docker API error: boom");
    }
}
