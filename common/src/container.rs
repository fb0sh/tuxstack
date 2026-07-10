use serde::{Deserialize, Serialize};

/// Represents a Docker container's state as seen by tuxstack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: ContainerStatus,
    pub created: String,
    pub ports: Vec<PortMapping>,
    pub cpu_usage: Option<f64>,
    pub memory_usage: Option<u64>,
    pub memory_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContainerStatus {
    Running,
    Exited,
    Paused,
    Restarting,
    Removing,
    Dead,
    Created,
    Unknown(String),
}

impl ContainerStatus {
    pub fn as_str(&self) -> &str {
        match self {
            ContainerStatus::Running => "running",
            ContainerStatus::Exited => "exited",
            ContainerStatus::Paused => "paused",
            ContainerStatus::Restarting => "restarting",
            ContainerStatus::Removing => "removing",
            ContainerStatus::Dead => "dead",
            ContainerStatus::Created => "created",
            ContainerStatus::Unknown(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub size: u64,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeProject {
    pub name: String,
    pub status: String,
    pub config_files: Vec<String>,
    pub containers: Vec<ContainerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLogs {
    pub container_id: String,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub stream: String, // stdout or stderr
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerAction {
    pub container_id: String,
    pub action: ContainerActionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContainerActionType {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
    Exec { command: Vec<String> },
}
