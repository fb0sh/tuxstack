//! Mapping for container Docker DTOs.

use bollard::container::LogOutput;
use bollard::models::{
    ContainerInspectResponse, ContainerState as BollardInspectState, ContainerStateStatusEnum,
    ContainerSummary as BollardSummary, ContainerSummaryStateEnum, Health as BollardHealth,
    HostConfig, MountPoint, NetworkSettings, PortSummary,
};
use chrono::{DateTime, TimeZone, Utc};

use crate::error::DockerError;
use crate::models::{
    ContainerDetail, ContainerRuntimeState, ContainerStateDetail, ContainerSummary,
    EnvironmentVariable, HealthLogEntry, HealthStatus, LogLine, LogStream, MountInfo,
    NetworkAttachment, PortBinding, ResourceLimits, RestartPolicy,
};

pub fn short_id(id: &str) -> String {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    let id = id.strip_prefix("sha256-").unwrap_or(id);
    if id.len() > 12 {
        id[..12].to_string()
    } else {
        id.to_string()
    }
}

/// Convert a Unix timestamp without inventing a current time fallback.
pub fn from_unix_seconds(secs: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(secs, 0).single()
}

fn unknown_timestamp() -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH
}

pub fn map_container_summary(summary: BollardSummary) -> ContainerSummary {
    let id = summary.id.unwrap_or_default();
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
        .map(map_summary_state)
        .unwrap_or(ContainerRuntimeState::Unknown);

    ContainerSummary {
        short_id: short_id(&id),
        id,
        name,
        image: summary.image.unwrap_or_default(),
        image_id: summary.image_id.unwrap_or_default(),
        state,
        status: summary.status.unwrap_or_default(),
        created_at: summary
            .created
            .and_then(from_unix_seconds)
            .unwrap_or_else(unknown_timestamp),
        ports: summary
            .ports
            .unwrap_or_default()
            .into_iter()
            .filter_map(map_port_summary)
            .collect(),
        labels: summary.labels.unwrap_or_default().into_iter().collect(),
    }
}

fn map_summary_state(state: &ContainerSummaryStateEnum) -> ContainerRuntimeState {
    match state {
        ContainerSummaryStateEnum::CREATED => ContainerRuntimeState::Created,
        ContainerSummaryStateEnum::RUNNING => ContainerRuntimeState::Running,
        ContainerSummaryStateEnum::PAUSED => ContainerRuntimeState::Paused,
        ContainerSummaryStateEnum::RESTARTING => ContainerRuntimeState::Restarting,
        ContainerSummaryStateEnum::REMOVING => ContainerRuntimeState::Removing,
        ContainerSummaryStateEnum::EXITED => ContainerRuntimeState::Exited,
        ContainerSummaryStateEnum::DEAD => ContainerRuntimeState::Dead,
        _ => ContainerRuntimeState::Unknown,
    }
}

pub fn map_port_summary(port: PortSummary) -> Option<PortBinding> {
    if port.private_port == 0 {
        return None;
    }
    Some(PortBinding {
        host_ip: port.ip,
        host_port: port.public_port,
        container_port: port.private_port,
        protocol: port
            .typ
            .map(|protocol| protocol.to_string())
            .filter(|protocol| !protocol.is_empty())
            .unwrap_or_else(|| "tcp".to_string()),
    })
}

pub fn map_container_detail(
    inspect: ContainerInspectResponse,
) -> Result<ContainerDetail, DockerError> {
    let config = inspect.config.as_ref();
    let host_config = inspect.host_config.as_ref();
    let state = inspect.state.as_ref();
    let id = inspect.id.clone().unwrap_or_default();
    let name = inspect
        .name
        .clone()
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string();
    let labels = config
        .and_then(|value| value.labels.as_ref())
        .map(|labels| labels.clone().into_iter().collect())
        .unwrap_or_default();
    let ports = map_inspect_ports(inspect.network_settings.as_ref());
    let mounts = inspect
        .mounts
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(map_mount_point)
        .collect::<Vec<_>>();
    let networks = map_network_attachments(inspect.network_settings.as_ref());
    let health = state.and_then(|value| value.health.clone()).map(map_health);
    let status = state
        .and_then(|value| value.status.as_ref())
        .map(ToString::to_string)
        .unwrap_or_default();

    let summary = ContainerSummary {
        short_id: short_id(&id),
        id,
        name,
        image: config
            .and_then(|value| value.image.clone())
            .unwrap_or_default(),
        image_id: inspect.image.clone().unwrap_or_default(),
        state: map_inspect_state(state),
        status,
        created_at: inspect.created.unwrap_or_else(unknown_timestamp),
        ports,
        labels,
    };

    let command = config
        .and_then(|value| value.cmd.clone())
        .unwrap_or_else(|| inspect.args.clone().unwrap_or_default());
    let entrypoint = config
        .and_then(|value| value.entrypoint.clone())
        .unwrap_or_else(|| inspect.path.clone().into_iter().collect());
    let environment = config
        .and_then(|value| value.env.as_ref())
        .map(|environment| {
            environment
                .iter()
                .map(|line| match line.split_once('=') {
                    Some((name, value)) => EnvironmentVariable {
                        name: name.to_string(),
                        value: Some(value.to_string()),
                    },
                    None => EnvironmentVariable {
                        name: line.to_string(),
                        value: None,
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    let restart_policy = host_config
        .and_then(|value| value.restart_policy.clone())
        .map(|policy| RestartPolicy {
            name: policy
                .name
                .map(|name| name.to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "no".to_string()),
            maximum_retry_count: policy
                .maximum_retry_count
                .and_then(|count| u64::try_from(count).ok()),
        })
        .unwrap_or(RestartPolicy {
            name: "no".to_string(),
            maximum_retry_count: None,
        });
    let state_detail = map_state_detail(state, inspect.restart_count);
    let resource_limits = host_config.map(map_resource_limits).unwrap_or_default();

    Ok(ContainerDetail {
        summary,
        command,
        entrypoint,
        environment,
        mounts,
        networks,
        restart_policy,
        health,
        platform: inspect.platform,
        hostname: config.and_then(|value| value.hostname.clone()),
        domain_name: config.and_then(|value| value.domainname.clone()),
        working_dir: config.and_then(|value| value.working_dir.clone()),
        user: config.and_then(|value| value.user.clone()),
        stop_signal: config.and_then(|value| value.stop_signal.clone()),
        stop_timeout_seconds: config.and_then(|value| value.stop_timeout),
        auto_remove: host_config
            .and_then(|value| value.auto_remove)
            .unwrap_or(false),
        tty: config.and_then(|value| value.tty).unwrap_or(false),
        open_stdin: config.and_then(|value| value.open_stdin).unwrap_or(false),
        read_only_rootfs: host_config
            .and_then(|value| value.readonly_rootfs)
            .unwrap_or(false),
        privileged: host_config
            .and_then(|value| value.privileged)
            .unwrap_or(false),
        state_detail,
        resource_limits,
    })
}

fn map_inspect_ports(settings: Option<&NetworkSettings>) -> Vec<PortBinding> {
    let mut result = Vec::new();
    let Some(port_map) = settings.and_then(|value| value.ports.as_ref()) else {
        return result;
    };
    for (key, bindings) in port_map {
        let Some((container_port, protocol)) = parse_port_string(key) else {
            continue;
        };
        match bindings {
            Some(bindings) if !bindings.is_empty() => {
                result.extend(bindings.iter().map(|binding| {
                    PortBinding {
                        host_ip: binding.host_ip.clone().filter(|value| !value.is_empty()),
                        host_port: binding
                            .host_port
                            .as_deref()
                            .filter(|value| !value.is_empty())
                            .and_then(|value| value.parse().ok()),
                        container_port,
                        protocol: protocol.clone(),
                    }
                }));
            }
            _ => result.push(PortBinding {
                host_ip: None,
                host_port: None,
                container_port,
                protocol,
            }),
        }
    }
    result.sort_by(|a, b| {
        a.container_port
            .cmp(&b.container_port)
            .then_with(|| a.protocol.cmp(&b.protocol))
            .then_with(|| a.host_ip.cmp(&b.host_ip))
            .then_with(|| a.host_port.cmp(&b.host_port))
    });
    result
}

fn map_inspect_state(state: Option<&BollardInspectState>) -> ContainerRuntimeState {
    match state {
        Some(value) => match value.status.as_ref() {
            Some(ContainerStateStatusEnum::CREATED) => ContainerRuntimeState::Created,
            Some(ContainerStateStatusEnum::RUNNING) => ContainerRuntimeState::Running,
            Some(ContainerStateStatusEnum::PAUSED) => ContainerRuntimeState::Paused,
            Some(ContainerStateStatusEnum::RESTARTING) => ContainerRuntimeState::Restarting,
            Some(ContainerStateStatusEnum::REMOVING) => ContainerRuntimeState::Removing,
            Some(ContainerStateStatusEnum::EXITED) => ContainerRuntimeState::Exited,
            Some(ContainerStateStatusEnum::DEAD) => ContainerRuntimeState::Dead,
            Some(_) => ContainerRuntimeState::Unknown,
            None if value.restarting == Some(true) => ContainerRuntimeState::Restarting,
            None if value.running == Some(true) && value.paused == Some(true) => {
                ContainerRuntimeState::Paused
            }
            None if value.running == Some(true) => ContainerRuntimeState::Running,
            None if value.dead == Some(true) => ContainerRuntimeState::Dead,
            None => ContainerRuntimeState::Exited,
        },
        None => ContainerRuntimeState::Unknown,
    }
}

fn map_state_detail(
    state: Option<&BollardInspectState>,
    restart_count: Option<i64>,
) -> ContainerStateDetail {
    ContainerStateDetail {
        running: state.and_then(|value| value.running).unwrap_or(false),
        paused: state.and_then(|value| value.paused).unwrap_or(false),
        restarting: state.and_then(|value| value.restarting).unwrap_or(false),
        oom_killed: state.and_then(|value| value.oom_killed).unwrap_or(false),
        dead: state.and_then(|value| value.dead).unwrap_or(false),
        exit_code: state.and_then(|value| value.exit_code),
        error: state
            .and_then(|value| value.error.clone())
            .filter(|value| !value.is_empty()),
        started_at: state
            .and_then(|value| value.started_at.as_deref())
            .and_then(parse_docker_datetime),
        finished_at: state
            .and_then(|value| value.finished_at.as_deref())
            .and_then(parse_docker_datetime),
        restart_count: restart_count
            .and_then(|count| u64::try_from(count).ok())
            .unwrap_or(0),
    }
}

fn parse_docker_datetime(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() || value.starts_with("0001-01-01") {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.with_timezone(&Utc))
}

fn parse_port_string(value: &str) -> Option<(u16, String)> {
    let (port, protocol) = value.split_once('/')?;
    Some((port.parse().ok()?, protocol.to_string()))
}

fn map_network_attachments(settings: Option<&NetworkSettings>) -> Vec<NetworkAttachment> {
    let mut result = settings
        .and_then(|value| value.networks.as_ref())
        .map(|networks| {
            networks
                .iter()
                .map(|(name, endpoint)| NetworkAttachment {
                    network_name: name.clone(),
                    network_id: endpoint.network_id.clone(),
                    ipv4: endpoint
                        .ip_address
                        .clone()
                        .filter(|value| !value.is_empty()),
                    ipv6: endpoint
                        .global_ipv6_address
                        .clone()
                        .filter(|value| !value.is_empty()),
                    gateway: endpoint.gateway.clone().filter(|value| !value.is_empty()),
                    ipv6_gateway: endpoint
                        .ipv6_gateway
                        .clone()
                        .filter(|value| !value.is_empty()),
                    mac: endpoint
                        .mac_address
                        .clone()
                        .filter(|value| !value.is_empty()),
                    aliases: endpoint.aliases.clone().unwrap_or_default(),
                    endpoint_id: endpoint
                        .endpoint_id
                        .clone()
                        .filter(|value| !value.is_empty()),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    result.sort_by(|a, b| a.network_name.cmp(&b.network_name));
    result
}

fn map_health(health: BollardHealth) -> HealthStatus {
    let log = health
        .log
        .unwrap_or_default()
        .into_iter()
        .map(|entry| HealthLogEntry {
            exit_code: entry.exit_code,
            output: entry.output,
            start: entry.start,
            end: entry.end,
        })
        .collect::<Vec<_>>();
    HealthStatus {
        status: health
            .status
            .map(|status| status.to_string())
            .unwrap_or_default(),
        failing_streak: health
            .failing_streak
            .and_then(|count| u64::try_from(count).ok()),
        last_check: log.last().and_then(|entry| entry.end.or(entry.start)),
        log,
    }
}

fn map_resource_limits(host_config: &HostConfig) -> ResourceLimits {
    ResourceLimits {
        memory_bytes: host_config
            .memory
            .and_then(|value| u64::try_from(value).ok()),
        nano_cpus: host_config
            .nano_cpus
            .and_then(|value| u64::try_from(value).ok()),
        pids_limit: host_config.pids_limit,
        cpu_shares: host_config.cpu_shares,
    }
}

pub fn map_log_output(output: LogOutput, timestamps: bool) -> LogLine {
    let (stream, raw) = match output {
        LogOutput::StdErr { message } => (LogStream::Stderr, message),
        LogOutput::StdOut { message } => (LogStream::Stdout, message),
        LogOutput::Console { message } => (LogStream::Console, message),
        LogOutput::StdIn { message } => (LogStream::Unknown, message),
    };
    let message = String::from_utf8_lossy(&raw).into_owned();
    let (timestamp, message) = if timestamps {
        if let Some((prefix, rest)) = message.split_once(' ') {
            (parse_docker_datetime(prefix), rest.to_string())
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

pub fn map_mount_point(mount: MountPoint) -> MountInfo {
    MountInfo {
        source: mount.source,
        destination: mount.destination.unwrap_or_default(),
        mode: mount.mode,
        rw: mount.rw.unwrap_or(false),
        mount_type: mount.typ.unwrap_or_default(),
        name: mount.name,
        propagation: mount.propagation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        ContainerConfig, ContainerSummary as BS, ContainerSummaryStateEnum, EndpointSettings,
        Health, HealthStatusEnum, HealthcheckResult, MountPoint, NetworkSettings,
        PortBinding as BP, PortSummary, PortSummaryTypeEnum,
    };
    use std::collections::HashMap;

    #[test]
    fn short_id_trims_sha256_prefix_and_length() {
        assert_eq!(short_id("sha256:abcdef1234567890"), "abcdef123456");
        assert_eq!(short_id("abcdef"), "abcdef");
    }

    #[test]
    fn summary_maps_basic_fields_and_unknown_timestamp() {
        let mapped = map_container_summary(BS {
            id: Some("0123456789abcdef0123456789abcdef".into()),
            names: Some(vec!["/web".into(), "/alias".into()]),
            image: Some("nginx:latest".into()),
            image_id: Some("sha256:image".into()),
            created: Some(1_700_000_000),
            state: Some(ContainerSummaryStateEnum::RUNNING),
            status: Some("Up 2 hours (healthy)".into()),
            ports: Some(vec![PortSummary {
                ip: Some("0.0.0.0".into()),
                private_port: 80,
                public_port: Some(8080),
                typ: Some(PortSummaryTypeEnum::TCP),
            }]),
            ..Default::default()
        });
        assert_eq!(mapped.name, "web");
        assert_eq!(mapped.state, ContainerRuntimeState::Running);
        assert_eq!(mapped.created_at_opt().unwrap().timestamp(), 1_700_000_000);
        assert_eq!(
            mapped.health_summary().unwrap().status,
            crate::models::ContainerHealthState::Healthy
        );
        assert_eq!(mapped.ports[0].display(), "0.0.0.0:8080->80/tcp");

        let unknown = map_container_summary(BS::default());
        assert_eq!(unknown.created_at_opt(), None);
        let out_of_range = map_container_summary(BS {
            created: Some(i64::MAX),
            ..Default::default()
        });
        assert_eq!(out_of_range.created_at_opt(), None);
    }

    #[test]
    fn zero_private_port_is_skipped() {
        assert!(map_port_summary(PortSummary::default()).is_none());
    }

    #[test]
    fn inspect_maps_health_ports_mounts_networks_and_configuration() {
        let inspect = ContainerInspectResponse {
            id: Some("abcdef1234567890".into()),
            image: Some("sha256:image-id".into()),
            restart_count: Some(3),
            config: Some(ContainerConfig {
                image: Some("nginx:latest".into()),
                hostname: Some("web".into()),
                domainname: Some("example.test".into()),
                user: Some("1000:1000".into()),
                tty: Some(true),
                open_stdin: Some(true),
                stop_signal: Some("SIGTERM".into()),
                stop_timeout: Some(30),
                env: Some(vec!["TOKEN=secret=value".into(), "UNSET".into()]),
                ..Default::default()
            }),
            state: Some(BollardInspectState {
                running: Some(true),
                oom_killed: Some(true),
                exit_code: Some(137),
                started_at: Some("2024-01-01T00:00:00Z".into()),
                finished_at: Some("0001-01-01T00:00:00Z".into()),
                health: Some(Health {
                    status: Some(HealthStatusEnum::UNHEALTHY),
                    failing_streak: Some(2),
                    log: Some(vec![HealthcheckResult {
                        start: None,
                        end: Some(
                            DateTime::parse_from_rfc3339("2024-01-01T00:00:01Z")
                                .unwrap()
                                .with_timezone(&Utc),
                        ),
                        exit_code: Some(1),
                        output: Some("failed".into()),
                    }]),
                }),
                ..Default::default()
            }),
            mounts: Some(vec![MountPoint {
                typ: Some("bind".into()),
                source: Some("/host".into()),
                destination: Some("/work".into()),
                rw: Some(false),
                propagation: Some("rshared".into()),
                ..Default::default()
            }]),
            network_settings: Some(NetworkSettings {
                ports: Some(HashMap::from([
                    (
                        "80/tcp".into(),
                        Some(vec![BP {
                            host_ip: Some("0.0.0.0".into()),
                            host_port: Some("8080".into()),
                        }]),
                    ),
                    ("443/tcp".into(), None),
                ])),
                networks: Some(HashMap::from([(
                    "frontend".into(),
                    EndpointSettings {
                        network_id: Some("network-id".into()),
                        endpoint_id: Some("endpoint-id".into()),
                        gateway: Some("172.20.0.1".into()),
                        ip_address: Some("172.20.0.2".into()),
                        aliases: Some(vec!["web".into()]),
                        ..Default::default()
                    },
                )])),
                ..Default::default()
            }),
            host_config: Some(HostConfig {
                auto_remove: Some(true),
                privileged: Some(true),
                readonly_rootfs: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let detail = map_container_detail(inspect).unwrap();
        assert_eq!(detail.summary.image, "nginx:latest");
        assert_eq!(detail.summary.image_id, "sha256:image-id");
        assert_eq!(detail.environment[0].value.as_deref(), Some("secret=value"));
        assert_eq!(detail.summary.ports.len(), 2);
        assert!(
            !detail
                .summary
                .ports
                .iter()
                .find(|port| port.container_port == 443)
                .unwrap()
                .is_published()
        );
        assert_eq!(detail.mounts[0].propagation.as_deref(), Some("rshared"));
        assert_eq!(detail.networks[0].gateway.as_deref(), Some("172.20.0.1"));
        assert_eq!(detail.networks[0].aliases, vec!["web"]);
        assert_eq!(
            detail
                .health
                .as_ref()
                .unwrap()
                .last_check
                .unwrap()
                .timestamp(),
            1_704_067_201
        );
        assert_eq!(detail.domain_name.as_deref(), Some("example.test"));
        assert_eq!(detail.user.as_deref(), Some("1000:1000"));
        assert!(detail.auto_remove && detail.tty && detail.open_stdin);
        assert!(detail.read_only_rootfs && detail.privileged);
        assert_eq!(detail.state_detail.restart_count, 3);
        assert!(detail.state_detail.oom_killed);
        assert!(detail.state_detail.finished_at.is_none());
    }

    #[test]
    fn inspect_state_boolean_fallback_is_precise() {
        assert_eq!(
            map_inspect_state(Some(&BollardInspectState {
                running: Some(true),
                paused: Some(true),
                ..Default::default()
            })),
            ContainerRuntimeState::Paused
        );
        assert_eq!(
            map_inspect_state(Some(&BollardInspectState {
                restarting: Some(true),
                ..Default::default()
            })),
            ContainerRuntimeState::Restarting
        );
    }

    #[test]
    fn log_timestamp_mapping_keeps_message() {
        let line = map_log_output(
            LogOutput::StdErr {
                message: "2024-01-01T00:00:00Z boom".into(),
            },
            true,
        );
        assert_eq!(line.stream, LogStream::Stderr);
        assert_eq!(line.message, "boom");
        assert!(line.timestamp.is_some());
    }
}
