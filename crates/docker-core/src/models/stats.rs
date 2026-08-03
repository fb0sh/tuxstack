//! Container statistics model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A sampled snapshot of container resource usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    pub pids: Option<u64>,
    pub sampled_at: DateTime<Utc>,
}

impl Default for ContainerStats {
    fn default() -> Self {
        Self {
            cpu_percent: 0.0,
            memory_usage_bytes: 0,
            memory_limit_bytes: 0,
            memory_percent: 0.0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            block_read_bytes: 0,
            block_write_bytes: 0,
            pids: None,
            sampled_at: Utc::now(),
        }
    }
}
