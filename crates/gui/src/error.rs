//! GUI-level errors derived from the typed daemon protocol.

use tuxstack_client::DaemonError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    DaemonUnavailable(String),
    DockerUnavailable,
    FuseUnavailable(String),
    PermissionDenied,
    Operation(String),
    #[allow(dead_code)]
    Config(String),
}

impl AppError {
    pub fn user_message(&self) -> String {
        match self {
            Self::DaemonUnavailable(_) => "TuxStack service is not running.".into(),
            Self::DockerUnavailable => "Docker Engine is unavailable.".into(),
            Self::FuseUnavailable(_) => "Docker filesystem is unavailable.".into(),
            Self::PermissionDenied => {
                "Permission denied while accessing the TuxStack service.".into()
            }
            Self::Operation(message) => message.clone(),
            Self::Config(message) => format!("Configuration error: {message}"),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::DaemonUnavailable(_) => "daemon_unavailable",
            Self::DockerUnavailable => "docker_unavailable",
            Self::FuseUnavailable(_) => "fuse_unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::Operation(_) => "daemon",
            Self::Config(_) => "config",
        }
    }

    pub fn status_code(&self) -> i32 {
        match self {
            Self::DaemonUnavailable(_) => 5,
            Self::DockerUnavailable => 2,
            Self::FuseUnavailable(_) => 6,
            Self::PermissionDenied => 3,
            Self::Operation(_) | Self::Config(_) => 4,
        }
    }
}

impl From<&DaemonError> for AppError {
    fn from(error: &DaemonError) -> Self {
        Self::from(error.clone())
    }
}

impl From<DaemonError> for AppError {
    fn from(error: DaemonError) -> Self {
        match error {
            DaemonError::DaemonUnavailable(message) => Self::DaemonUnavailable(message),
            DaemonError::SocketNotFound(path) => {
                Self::DaemonUnavailable(path.display().to_string())
            }
            DaemonError::EngineUnavailable | DaemonError::ConnectionTimeout => {
                Self::DockerUnavailable
            }
            DaemonError::FuseUnavailable(message) => Self::FuseUnavailable(message),
            DaemonError::PermissionDenied | DaemonError::DestinationPermissionDenied(_) => {
                Self::PermissionDenied
            }
            other => Self::Operation(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_docker_and_fuse_statuses_are_distinct() {
        assert_eq!(
            AppError::from(DaemonError::DaemonUnavailable("offline".into())).status_code(),
            5
        );
        assert_eq!(
            AppError::from(DaemonError::EngineUnavailable).status_code(),
            2
        );
        assert_eq!(
            AppError::from(DaemonError::FuseUnavailable("unmounted".into())).status_code(),
            6
        );
    }
}
