//! Mapping for container DTOs.

use bollard::models::{
    ContainerInspectResponse, ContainerState as BollardInspectState,
    ContainerStateStatusEnum, ContainerSummary as BollardSummary,
    ContainerSummaryStateEnum, Health as BollardHealth, HostConfig, MountPoint,
    NetworkSettings, PortSummary,
};
use bollard::container::LogOutput;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::DockerError;
use crate::models::{
    ContainerDetail, ContainerState, ContainerSummary, EnvironmentVariable, HealthLogEntry,
    HealthStatus, LogLine, LogStream, MountInfo, NetworkAttachment, PortBinding, ResourceLimits,
    RestartPolicy,
};

/// Extract the short (12 char) id from a full id, trimming a `sha256:` prefix.
pub fn short_id(id: &str) -> String {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    let id = id.strip_prefix("sha256-").unwrap_or(id);
    if id.len() > 12 {
        id[..12].to_string()
    } else {
        id.to_string()
    }
}

/// Convert a unix timestamp (seconds) into a UTC datetime.
pub fn from_unix_seconds(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

/// Map a bollard list-entry container summary.
pub fn map_container_summary(summary: BollardSummary) -> ContainerSummary {
    let id = summary.id.unwrap_or_default();
    let short = short_id(&id);
    let name = summary
        .names
        .as_deref()
        .and_then(|names| names.first())
        .cloned()
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string();

    let state = summary
        .state
        .as_ref()
        .map(|s| match s {
            ContainerSummaryStateEnum::CREATED => ContainerState::Created,
            ContainerSummaryStateEnum::RUNNING => ContainerState::Running,
            ContainerSummaryStateEnum::PAUSED => ContainerState::Paused,
            ContainerSummaryStateEnum::RESTARTING => ContainerState::Restarting,
            ContainerSummaryStateEnum::REMOVING => ContainerState::Removing,
            ContainerSummaryStateEnum::EXITED => ContainerState::Exited,
            ContainerSummaryStateEnum::DEAD => ContainerState::Dead,
            _ => ContainerState::Unknown,
        })
        .unwrap_or(ContainerState::Unknown);

    let ports = summary
        .ports
        .unwrap_or_default()
        .into_iter()
        .filter_map(map_port_summary)
        .collect();

    ContainerSummary {
        id,
        short_id: short,
        name,
        image: summary.image.unwrap_or_default(),
        image_id: summary.image_id.unwrap_or_default(),
        state,
        status: summary.status.unwrap_or_default(),
        created_at: summary
            .created
            .map(from_unix_seconds)
            .unwrap_or_else(Utc::now),
        ports,
        labels: summary.labels.unwrap_or_default().into_iter().collect(),
    }
}

/// Map a single port summary entry; returns `None` for empty/noise entries.
pub fn map_port_summary(port: PortSummary) -> Option<PortBinding> {
    let protocol = match port.typ {
        Some(ref t) => t.to_string(),
        None => "tcp".to_string(),
    };
    let container_port = port.private_port;
    if container_port == 0 {
        return None;
    }
    Some(PortBinding {
        host_ip: port.ip,
        host_port: port.public_port,
        container_port,
        protocol,
    })
}

/// Map a bollard container inspect response into the domain detail model.
pub fn map_container_detail(
    inspect: ContainerInspectResponse,
) -> Result<ContainerDetail, DockerError> {
    let summary = ContainerSummary {
        id: inspect.id.clone().unwrap_or_default(),
        short_id: short_id(&inspect.id.clone().unwrap_or_default()),
        name: inspect
            .name
            .clone()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image: inspect.image.clone().unwrap_or_default(),
        image_id: String::new(),
        state: map_inspect_state(inspect.state.as_ref()),
        status: inspect
            .state
            .as_ref()
            .and_then(|s| s.status.as_ref())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        created_at: inspect.created.unwrap_or_else(Utc::now),
        ports: inspect
            .network_settings
            .as_ref()
            .and_then(|n| n.ports.as_ref())
            .map(|port_map| {
                port_map
                    .iter()
                    .flat_map(|(key, values)| {
                        values.iter().flatten().filter_map(|p| {
                            parse_port_string(key).map(|(container_port, protocol)| PortBinding {
                                host_ip: p.host_ip.clone(),
                                host_port: p.host_port.as_ref().and_then(|h| h.parse().ok()),
                                container_port,
                                protocol,
                            })
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        labels: inspect
            .config
            .as_ref()
            .and_then(|c| c.labels.as_ref())
            .map(|l| l.clone().into_iter().collect())
            .unwrap_or_default(),
    };

    let (command, entrypoint) = match inspect.config.as_ref() {
        Some(config) => (
            config.cmd.clone().unwrap_or_default(),
            config.entrypoint.clone().unwrap_or_default(),
        ),
        None => (Vec::new(), Vec::new()),
    };

    let environment = inspect
        .config
        .as_ref()
        .and_then(|c| c.env.as_ref())
        .map(|env| {
            env.iter()
                .filter_map(|line| {
                    let (name, value) = line.split_once('=')?;
                    Some(EnvironmentVariable {
                        name: name.to_string(),
                        value: Some(value.to_string()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mounts = inspect
        .mounts
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|m| MountInfo {
            source: m.source.clone(),
            destination: m.destination.clone().unwrap_or_default(),
            mode: m.mode.clone(),
            rw: m.rw.unwrap_or(false),
            mount_type: m
                .typ
                .clone()
                .map(|t| t.to_string())
                .unwrap_or_default(),
        })
        .collect();

    let networks = map_network_attachments(inspect.network_settings.as_ref());

    let restart_policy = inspect
        .host_config
        .as_ref()
        .and_then(|h| h.restart_policy.clone())
        .map(|p| RestartPolicy {
            name: p.name.map(|n| n.to_string()).unwrap_or_default(),
            maximum_retry_count: p.maximum_retry_count.map(|c| c as u64),
        })
        .unwrap_or(RestartPolicy {
            name: "no".to_string(),
            maximum_retry_count: None,
        });

    let health = inspect
        .state
        .as_ref()
        .and_then(|s| s.health.clone())
        .map(|h| map_health(h));

    let resource_limits = inspect
        .host_config
        .as_ref()
        .map(|h| map_resource_limits(h))
        .unwrap_or_default();

    Ok(ContainerDetail {
        summary,
        command,
        entrypoint,
        environment,
        mounts,
        networks,
        restart_policy,
        health,
        platform: inspect.platform.clone(),
        hostname: inspect
            .config
            .as_ref()
            .and_then(|c| c.hostname.clone()),
        working_dir: inspect
            .config
            .as_ref()
            .and_then(|c| c.working_dir.clone()),
        resource_limits,
    })
}

fn map_inspect_state(state: Option<&BollardInspectState>) -> ContainerState {
    match state {
        Some(s) => match s.status.as_ref() {
            Some(status) => match status {
                ContainerStateStatusEnum::CREATED => ContainerState::Created,
                ContainerStateStatusEnum::RUNNING => ContainerState::Running,
                ContainerStateStatusEnum::PAUSED => ContainerState::Paused,
                ContainerStateStatusEnum::RESTARTING => ContainerState::Restarting,
                ContainerStateStatusEnum::REMOVING => ContainerState::Removing,
                ContainerStateStatusEnum::EXITED => ContainerState::Exited,
                ContainerStateStatusEnum::DEAD => ContainerState::Dead,
                _ => ContainerState::Unknown,
            },
            None => {
                if s.running == Some(true) && s.paused == Some(true) {
                    ContainerState::Paused
                } else if s.running == Some(true) {
                    ContainerState::Running
                } else if s.dead == Some(true) {
                    ContainerState::Dead
                } else {
                    ContainerState::Exited
                }
            }
        },
        None => ContainerState::Unknown,
    }
}

fn parse_port_string(s: &str) -> Option<(u16, String)> {
    // Docker port map keys look like "80/tcp" or "53/udp".
    let (port, proto) = s.split_once('/')?;
    let port: u16 = port.parse().ok()?;
    Some((port, proto.to_string()))
}

fn map_network_attachments(settings: Option<&NetworkSettings>) -> Vec<NetworkAttachment> {
    settings
        .and_then(|s| s.networks.as_ref())
        .map(|networks| {
            networks
                .iter()
                .map(|(name, ep)| NetworkAttachment {
                    network_name: name.clone(),
                    ipv4: ep.ip_address.clone(),
                    ipv6: ep.global_ipv6_address.clone(),
                    mac: ep.mac_address.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn map_health(health: BollardHealth) -> HealthStatus {
    HealthStatus {
        status: health
            .status
            .map(|s| s.to_string())
            .unwrap_or_default(),
        failing_streak: health.failing_streak.map(|c| c as u64),
        log: health
            .log
            .unwrap_or_default()
            .into_iter()
            .map(|entry| HealthLogEntry {
                exit_code: entry.exit_code,
                output: entry.output,
                start: entry.start,
                end: entry.end,
            })
            .collect(),
    }
}

fn map_resource_limits(host_config: &HostConfig) -> ResourceLimits {
    ResourceLimits {
        memory_bytes: host_config.memory.map(|m| m.max(0) as u64),
        nano_cpus: host_config.nano_cpus.map(|n| n.max(0) as u64),
        pids_limit: host_config.pids_limit,
        cpu_shares: host_config.cpu_shares,
    }
}

/// Map a bollard log output item into a domain log line.
pub fn map_log_output(output: LogOutput, timestamps: bool) -> LogLine {
    let (stream, raw) = match output {
        LogOutput::StdErr { message } => (LogStream::Stderr, message),
        LogOutput::StdOut { message } => (LogStream::Stdout, message),
        LogOutput::Console { message } => (LogStream::Console, message),
        LogOutput::StdIn { message } => (LogStream::Unknown, message),
    };
    let message = String::from_utf8_lossy(&raw).into_owned();

    // When timestamps are requested, Docker prefixes each line with an
    // RFC3339 timestamp followed by a space.
    let (timestamp, message) = if timestamps {
        if let Some((ts, rest)) = message.split_once(' ') {
            let parsed = DateTime::parse_from_rfc3339(ts)
                .map(|dt| dt.with_timezone(&Utc))
                .ok();
            (parsed, rest.to_string())
        } else {
            (None, message)
        }
    } else {
        (None, message)
    };

    LogLine {
        timestamp,
        stream,
        message,
    }
}

/// Map a bollard `ContainerSummary` for list display.
pub fn map_mount_point(m: MountPoint) -> MountInfo {
    MountInfo {
        source: m.source.clone(),
        destination: m.destination.clone().unwrap_or_default(),
        mode: m.mode.clone(),
        rw: m.rw.unwrap_or(false),
        mount_type: m
            .typ
            .clone()
            .map(|t| t.to_string())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        ContainerSummaryStateEnum, ContainerSummary as BS, PortSummary, PortSummaryTypeEnum,
    };

    #[test]
    fn short_id_trims_sha256_prefix_and_length() {
        assert_eq!(short_id("abcdef1234567890"), "abcdef123456");
        assert_eq!(short_id("sha256:abcdef1234567890"), "abcdef123456");
        assert_eq!(short_id("abcdef"), "abcdef");
        assert_eq!(short_id(""), "");
    }

    #[test]
    fn summary_maps_basic_fields() {
        let s = BS {
            id: Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into()),
            names: Some(vec!["/web".into(), "/web_alias".into()]),
            image: Some("nginx:latest".into()),
            image_id: Some("sha256:abc".into()),
            created: Some(1_700_000_000),
            state: Some(ContainerSummaryStateEnum::RUNNING),
            status: Some("Up 2 hours".into()),
            ..Default::default()
        };
        let mapped = map_container_summary(s);
        assert_eq!(mapped.name, "web");
        assert_eq!(mapped.state, ContainerState::Running);
        assert_eq!(mapped.status, "Up 2 hours");
        assert_eq!(mapped.created_at.timestamp(), 1_700_000_000);
        assert_eq!(mapped.short_id.len(), 12);
    }

    #[test]
    fn empty_summary_does_not_panic() {
        let mapped = map_container_summary(BS::default());
        assert_eq!(mapped.id, "");
        assert_eq!(mapped.name, "");
        assert_eq!(mapped.state, ContainerState::Unknown);
    }

    #[test]
    fn port_summary_mapping() {
        let p = PortSummary {
            ip: Some("0.0.0.0".into()),
            private_port: 8080,
            public_port: Some(80),
            typ: Some(PortSummaryTypeEnum::TCP),
        };
        let binding = map_port_summary(p).unwrap();
        assert_eq!(binding.host_ip.as_deref(), Some("0.0.0.0"));
        assert_eq!(binding.host_port, Some(80));
        assert_eq!(binding.container_port, 8080);
        assert_eq!(binding.protocol, "tcp");
        assert_eq!(binding.display(), "0.0.0.0:80->8080/tcp");
    }

    #[test]
    fn zero_private_port_is_skipped() {
        let p = PortSummary {
            private_port: 0,
            ..Default::default()
        };
        assert!(map_port_summary(p).is_none());
    }

    #[test]
    fn state_from_str() {
        assert_eq!(ContainerState::from_str_opt("running"), ContainerState::Running);
        assert_eq!(ContainerState::from_str_opt("exited"), ContainerState::Exited);
        assert_eq!(ContainerState::from_str_opt("weird"), ContainerState::Unknown);
        assert!(ContainerState::Running.is_active());
        assert!(!ContainerState::Exited.is_active());
    }

    #[test]
    fn log_output_mapping_without_timestamps() {
        let line = map_log_output(LogOutput::StdOut {
            message: "hello".into(),
        }, false);
        assert_eq!(line.stream, LogStream::Stdout);
        assert_eq!(line.message, "hello");
        assert!(line.timestamp.is_none());
    }

    #[test]
    fn log_output_mapping_with_timestamps() {
        let line = map_log_output(
            LogOutput::StdErr {
                message: "2024-01-01T00:00:00.000000000Z boom".into(),
            },
            true,
        );
        assert_eq!(line.stream, LogStream::Stderr);
        assert_eq!(line.message, "boom");
        assert!(line.timestamp.is_some());
    }

    #[test]
    fn log_output_mapping_bad_timestamp_keeps_message() {
        let line = map_log_output(
            LogOutput::StdOut {
                message: "not-a-timestamp hello".into(),
            },
            true,
        );
        assert_eq!(line.message, "hello");
        assert!(line.timestamp.is_none());
    }

    #[test]
    fn inspect_state_fallback_running() {
        let state = bollard::models::ContainerState {
            running: Some(true),
            paused: Some(false),
            ..Default::default()
        };
        assert_eq!(map_inspect_state(Some(&state)), ContainerState::Running);
    }

    #[test]
    fn inspect_state_fallback_paused() {
        let state = bollard::models::ContainerState {
            running: Some(true),
            paused: Some(true),
            ..Default::default()
        };
        assert_eq!(map_inspect_state(Some(&state)), ContainerState::Paused);
    }

    #[test]
    fn detail_maps_with_minimal_inspect() {
        let detail = map_container_detail(ContainerInspectResponse::default()).unwrap();
        assert_eq!(detail.summary.id, "");
        assert!(detail.environment.is_empty());
        assert!(detail.mounts.is_empty());
        assert_eq!(detail.restart_policy.name, "no");
    }
}
