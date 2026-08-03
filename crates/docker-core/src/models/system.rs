//! Docker Engine and system-level models.

use serde::{Deserialize, Serialize};

/// Information about the connected Docker Engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockerSystemInfo {
    pub version: String,
    pub api_version: String,
    pub min_api_version: String,
    pub os: String,
    pub arch: String,
    pub kernel_version: String,
    pub operating_system: String,
    pub server_version: String,
    pub docker_root_dir: String,
    pub total_memory: u64,
    pub n_cpus: u64,
    pub name: String,
    pub driver: String,
    pub containers: u64,
    pub containers_running: u64,
    pub containers_paused: u64,
    pub containers_stopped: u64,
    pub images: u64,
}

/// Aggregated data shown on the Overview page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewData {
    pub system: DockerSystemInfo,
    pub networks: u64,
    pub volumes: u64,
}

impl DockerSystemInfo {
    /// Total memory formatted as bytes for display.
    pub fn total_memory_bytes(&self) -> u64 {
        self.total_memory
    }
}
