//! CLI error type with exit-code mapping.

use tuxstack_docker_core::DockerError;

/// Exit codes documented in the README.
pub mod exit {
    pub const OK: u8 = 0;
    pub const GENERAL: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const DOCKER_UNAVAILABLE: u8 = 3;
    pub const PERMISSION_DENIED: u8 = 4;
    pub const NOT_FOUND: u8 = 5;
    pub const CONFLICT: u8 = 6;
    pub const TIMEOUT: u8 = 7;
}

/// Errors that terminate the CLI with a specific exit code.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Docker(#[from] DockerError),

    #[error("JSON output error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    #[allow(dead_code)] // reserved for future argument validation
    Usage(String),
}

impl CliError {
    /// The process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Docker(err) => match err {
                DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => {
                    exit::DOCKER_UNAVAILABLE
                }
                DockerError::PermissionDenied => exit::PERMISSION_DENIED,
                DockerError::ContainerNotFound(_)
                | DockerError::ImageNotFound(_)
                | DockerError::NetworkNotFound(_)
                | DockerError::VolumeNotFound(_) => exit::NOT_FOUND,
                DockerError::Conflict(_) => exit::CONFLICT,
                DockerError::ConnectionTimeout | DockerError::OperationTimeout => exit::TIMEOUT,
                _ => exit::GENERAL,
            },
            CliError::Json(_) | CliError::Io(_) => exit::GENERAL,
            CliError::Usage(_) => exit::USAGE,
        }
    }
}
