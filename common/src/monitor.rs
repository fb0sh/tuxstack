use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub docker_available: bool,
    pub incus_available: bool,
    pub docker_version: Option<String>,
    pub incus_version: Option<String>,
    pub containers_running: u32,
    pub containers_total: u32,
    pub instances_running: u32,
    pub instances_total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub container_id: String,
    pub cpu_percent: f64,
    pub memory_usage_bytes: u64,
    pub memory_limit_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub pids_current: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub total_cpu_percent: f64,
    pub total_memory_usage_bytes: u64,
    pub total_memory_limit_bytes: u64,
}
