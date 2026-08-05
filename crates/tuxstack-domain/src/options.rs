//! Operation options passed to Docker services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Options for stopping a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopContainerOptions {
    /// Grace period in seconds before SIGKILL.
    pub timeout_seconds: Option<i64>,
}

/// Options for removing a container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveContainerOptions {
    pub force: bool,
    pub remove_volumes: bool,
    pub remove_links: bool,
}

/// Options for reading container logs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerLogsOptions {
    pub stdout: bool,
    pub stderr: bool,
    pub timestamps: bool,
    pub follow: bool,
    pub tail: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

impl ContainerLogsOptions {
    /// Default historical log request: both streams, last 500 lines.
    pub fn historical(tail: usize) -> Self {
        Self {
            stdout: true,
            stderr: true,
            timestamps: false,
            follow: false,
            tail: Some(tail),
            since: None,
            until: None,
        }
    }

    /// Default follow request: both streams, follow mode, no tail limit.
    pub fn follow() -> Self {
        Self {
            stdout: true,
            stderr: true,
            timestamps: false,
            follow: true,
            tail: None,
            since: None,
            until: None,
        }
    }
}
