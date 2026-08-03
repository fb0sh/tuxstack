//! Mapping and computation for container stats.

use bollard::models::ContainerStatsResponse;
use chrono::Utc;

use crate::models::ContainerStats;

/// Compute CPU and memory percentages from a Docker stats sample.
///
/// The CPU percentage is computed relative to the previous sample; the
/// caller keeps track of the previous values.
pub fn compute_cpu_percent(
    current_total: u64,
    previous_total: u64,
    current_system: u64,
    previous_system: u64,
    online_cpus: u64,
) -> f64 {
    let cpu_delta = current_total.saturating_sub(previous_total);
    let system_delta = current_system.saturating_sub(previous_system);
    if cpu_delta == 0 || system_delta == 0 {
        return 0.0;
    }
    let cpus = online_cpus.max(1) as f64;
    (cpu_delta as f64 / system_delta as f64) * cpus * 100.0
}

/// Map a single Docker stats response into the domain model.
///
/// `previous` is the previous sample used for CPU calculation; pass
/// `None` for the first sample (CPU will be 0.0).
pub fn map_container_stats(
    response: ContainerStatsResponse,
    previous: Option<&ContainerStats>,
) -> ContainerStats {
    let cpu = response.cpu_stats.as_ref();
    let precpu = response.precpu_stats.as_ref();

    let total_usage = cpu.and_then(|c| c.cpu_usage.as_ref()).and_then(|u| u.total_usage);
    let prev_total = precpu
        .and_then(|c| c.cpu_usage.as_ref())
        .and_then(|u| u.total_usage);
    let system_usage = cpu.and_then(|c| c.system_cpu_usage);
    let prev_system = precpu.and_then(|c| c.system_cpu_usage);
    let online_cpus = cpu
        .and_then(|c| c.online_cpus)
        .unwrap_or(0) as u64;

    let cpu_percent = match (total_usage, prev_total, system_usage, prev_system) {
        (Some(cur), Some(prev), Some(cur_sys), Some(prev_sys)) => {
            compute_cpu_percent(cur, prev, cur_sys, prev_sys, online_cpus)
        }
        _ => previous.map(|p| p.cpu_percent).unwrap_or(0.0),
    };

    let memory = response.memory_stats.as_ref();
    let usage = memory.and_then(|m| m.usage).unwrap_or(0);
    let limit = memory.and_then(|m| m.limit).unwrap_or(0);
    let memory_percent = if limit > 0 {
        (usage as f64 / limit as f64) * 100.0
    } else {
        0.0
    };

    let (rx, tx) = response
        .networks
        .as_ref()
        .map(|nets| {
            nets.values().fold((0u64, 0u64), |(r, t), n| {
                (
                    r + n.rx_bytes.unwrap_or(0),
                    t + n.tx_bytes.unwrap_or(0),
                )
            })
        })
        .unwrap_or((0, 0));

    let (read_bytes, write_bytes) = response
        .blkio_stats
        .as_ref()
        .and_then(|b| b.io_service_bytes_recursive.as_ref())
        .map(|entries| {
            entries.iter().fold((0u64, 0u64), |(r, w), e| {
                match e.op.as_deref() {
                    Some("Read") => (r + e.value.unwrap_or(0), w),
                    Some("Write") => (r, w + e.value.unwrap_or(0)),
                    _ => (r, w),
                }
            })
        })
        .unwrap_or((0, 0));

    ContainerStats {
        cpu_percent,
        memory_usage_bytes: usage,
        memory_limit_bytes: limit,
        memory_percent,
        network_rx_bytes: rx,
        network_tx_bytes: tx,
        block_read_bytes: read_bytes,
        block_write_bytes: write_bytes,
        pids: response
            .pids_stats
            .as_ref()
            .and_then(|p| p.current),
        sampled_at: response.read.unwrap_or_else(Utc::now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        ContainerBlkioStats, ContainerBlkioStatEntry, ContainerCpuStats, ContainerCpuUsage,
        ContainerMemoryStats, ContainerNetworkStats, ContainerPidsStats, ContainerStatsResponse,
    };
    use std::collections::HashMap;

    fn cpu_stats(total: u64, system: u64, online: u32) -> ContainerCpuStats {
        ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage {
                total_usage: Some(total),
                ..Default::default()
            }),
            system_cpu_usage: Some(system),
            online_cpus: Some(online),
            ..Default::default()
        }
    }

    fn sample_response(cur: u64, prev: u64) -> ContainerStatsResponse {
        ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(cur, 10_000, 8)),
            precpu_stats: Some(cpu_stats(prev, 9_000, 8)),
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(50_000_000),
                limit: Some(100_000_000),
                ..Default::default()
            }),
            pids_stats: Some(ContainerPidsStats {
                current: Some(7),
                ..Default::default()
            }),
            blkio_stats: Some(ContainerBlkioStats {
                io_service_bytes_recursive: Some(vec![
                    ContainerBlkioStatEntry {
                        op: Some("Read".into()),
                        value: Some(1024),
                        ..Default::default()
                    },
                    ContainerBlkioStatEntry {
                        op: Some("Write".into()),
                        value: Some(2048),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            networks: Some(
                vec![(
                    "eth0".into(),
                    ContainerNetworkStats {
                        rx_bytes: Some(1000),
                        tx_bytes: Some(2000),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect::<HashMap<_, _>>(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn computes_cpu_percent() {
        // 1000 cpu units over 1000 system units on 8 cpus → 800%
        let pct = compute_cpu_percent(2000, 1000, 11_000, 10_000, 8);
        assert!((pct - 800.0).abs() < 0.001);
    }

    #[test]
    fn zero_deltas_give_zero() {
        assert_eq!(compute_cpu_percent(0, 0, 0, 0, 4), 0.0);
        assert_eq!(compute_cpu_percent(100, 100, 100, 100, 4), 0.0);
    }

    #[test]
    fn maps_full_stats_sample() {
        let mapped = map_container_stats(sample_response(2000, 1000), None);
        assert!(mapped.cpu_percent > 0.0);
        assert_eq!(mapped.memory_usage_bytes, 50_000_000);
        assert_eq!(mapped.memory_limit_bytes, 100_000_000);
        assert!((mapped.memory_percent - 50.0).abs() < 0.001);
        assert_eq!(mapped.network_rx_bytes, 1000);
        assert_eq!(mapped.network_tx_bytes, 2000);
        assert_eq!(mapped.block_read_bytes, 1024);
        assert_eq!(mapped.block_write_bytes, 2048);
        assert_eq!(mapped.pids, Some(7));
    }

    #[test]
    fn missing_networks_and_blkio_are_zero() {
        let resp = ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(2000, 10_000, 4)),
            precpu_stats: Some(cpu_stats(1000, 9_000, 4)),
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(10),
                limit: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mapped = map_container_stats(resp, None);
        assert_eq!(mapped.network_rx_bytes, 0);
        assert_eq!(mapped.block_read_bytes, 0);
        assert_eq!(mapped.pids, None);
    }

    #[test]
    fn first_sample_reports_zero_cpu() {
        let resp = ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(100, 10_000, 4)),
            ..Default::default()
        };
        let mapped = map_container_stats(resp, None);
        assert_eq!(mapped.cpu_percent, 0.0);
    }

    #[test]
    fn empty_response_is_safe() {
        let mapped = map_container_stats(ContainerStatsResponse::default(), None);
        assert_eq!(mapped.memory_usage_bytes, 0);
        assert_eq!(mapped.cpu_percent, 0.0);
    }
}
