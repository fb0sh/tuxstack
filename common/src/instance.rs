use serde::{Deserialize, Serialize};

/// Represents an Incus instance (container or VM).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub name: String,
    pub status: InstanceStatus,
    pub instance_type: InstanceType,
    pub image: String,
    pub created: String,
    pub cpu_usage: Option<f64>,
    pub memory_usage: Option<u64>,
    pub memory_limit: Option<u64>,
    pub ipv4: Vec<String>,
    pub snapshots: Vec<SnapshotInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstanceStatus {
    Running,
    Stopped,
    Frozen,
    Error,
    Unknown(String),
}

impl InstanceStatus {
    pub fn as_str(&self) -> &str {
        match self {
            InstanceStatus::Running => "running",
            InstanceStatus::Stopped => "stopped",
            InstanceStatus::Frozen => "frozen",
            InstanceStatus::Error => "error",
            InstanceStatus::Unknown(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstanceType {
    Container,
    VirtualMachine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub name: String,
    pub created: String,
    pub size: u64,
    pub stateful: bool,
}
