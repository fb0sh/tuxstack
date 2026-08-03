//! Container domain models.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A short list entry for a container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerSummary {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub state: ContainerState,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub ports: Vec<PortBinding>,
    pub labels: BTreeMap<String, String>,
}

impl ContainerSummary {
    /// Return the first 12 characters of the container ID.
    pub fn short_id(&self) -> &str {
        &self.short_id
    }
}

/// Container lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

impl ContainerState {
    /// Parse a Docker container state string into a typed state.
    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "created" => ContainerState::Created,
            "running" => ContainerState::Running,
            "paused" => ContainerState::Paused,
            "restarting" => ContainerState::Restarting,
            "removing" => ContainerState::Removing,
            "exited" => ContainerState::Exited,
            "dead" => ContainerState::Dead,
            _ => ContainerState::Unknown,
        }
    }

    /// Machine-readable string, used for filtering and output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerState::Created => "created",
            ContainerState::Running => "running",
            ContainerState::Paused => "paused",
            ContainerState::Restarting => "restarting",
            ContainerState::Removing => "removing",
            ContainerState::Exited => "exited",
            ContainerState::Dead => "dead",
            ContainerState::Unknown => "unknown",
        }
    }

    /// Whether the container is considered active (not exited/created/dead).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ContainerState::Running | ContainerState::Paused | ContainerState::Restarting
        )
    }
}

/// A host→container port mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBinding {
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
    pub container_port: u16,
    pub protocol: String,
}

impl PortBinding {
    /// Human readable form, e.g. `0.0.0.0:8080->80/tcp`.
    pub fn display(&self) -> String {
        let host = match (&self.host_ip, self.host_port) {
            (Some(ip), Some(port)) => format!("{ip}:{port}"),
            (None, Some(port)) => port.to_string(),
            _ => String::new(),
        };
        if host.is_empty() {
            format!("{}/{}", self.container_port, self.protocol)
        } else {
            format!("{host}->{}/{}", self.container_port, self.protocol)
        }
    }
}

/// Detailed view of a container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerDetail {
    pub summary: ContainerSummary,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub environment: Vec<EnvironmentVariable>,
    pub mounts: Vec<MountInfo>,
    pub networks: Vec<NetworkAttachment>,
    pub restart_policy: RestartPolicy,
    pub health: Option<HealthStatus>,
    pub platform: Option<String>,
    pub hostname: Option<String>,
    pub working_dir: Option<String>,
    pub resource_limits: ResourceLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: Option<String>,
}

impl EnvironmentVariable {
    /// Safe display form. Values are shown but callers must never log them
    /// if they may contain secrets.
    pub fn display(&self) -> String {
        match &self.value {
            Some(v) => format!("{}={}", self.name, v),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountInfo {
    pub source: Option<String>,
    pub destination: String,
    pub mode: Option<String>,
    pub rw: bool,
    pub mount_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAttachment {
    pub network_name: String,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub mac: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub name: String,
    pub maximum_retry_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub failing_streak: Option<u64>,
    pub log: Vec<HealthLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthLogEntry {
    pub exit_code: Option<i64>,
    pub output: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory_bytes: Option<u64>,
    pub nano_cpus: Option<u64>,
    pub pids_limit: Option<i64>,
    pub cpu_shares: Option<i64>,
}

/// Log output stream channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    Console,
    Unknown,
}

/// A single log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: Option<DateTime<Utc>>,
    pub stream: LogStream,
    pub message: String,
}
