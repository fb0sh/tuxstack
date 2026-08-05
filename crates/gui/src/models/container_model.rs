//! Qt-free view adapters for the unified Containers page.
//!
//! Docker domain objects stay in `docker-core`; this module only prepares
//! stable, structured values for the list and Info views. Environment values
//! are retained in memory, redacted from `Debug`, and masked until explicitly
//! revealed one row at a time.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use tuxstack_docker_core::{
    ContainerDetail, ContainerGroupSection, ContainerGroupSummary, ContainerHealthState,
    ContainerMountType, ContainerOperationState, ContainerRuntimeState, ContainerSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContainerSection {
    Running,
    Paused,
    Restarting,
    Stopped,
}

impl ContainerSection {
    pub const DISPLAY_ORDER: [Self; 4] =
        [Self::Running, Self::Paused, Self::Restarting, Self::Stopped];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Restarting => "restarting",
            Self::Stopped => "stopped",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Restarting => "Restarting",
            Self::Stopped => "Stopped",
        }
    }

    pub fn from_state(state: ContainerRuntimeState) -> Self {
        match state {
            ContainerRuntimeState::Running => Self::Running,
            ContainerRuntimeState::Paused => Self::Paused,
            ContainerRuntimeState::Restarting => Self::Restarting,
            _ => Self::Stopped,
        }
    }

    pub fn from_group(group: &ContainerGroupSummary) -> Self {
        match group.section() {
            ContainerGroupSection::Running => Self::Running,
            ContainerGroupSection::Paused => Self::Paused,
            ContainerGroupSection::Restarting => Self::Restarting,
            ContainerGroupSection::Stopped => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerRowKind {
    SectionHeader,
    Group,
    ContainerChild,
    Individual,
}

impl ContainerRowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SectionHeader => "section",
            Self::Group => "group",
            Self::ContainerChild => "container_child",
            Self::Individual => "individual",
        }
    }
}

/// One flattened visible list row. Section and group rows intentionally share
/// the same role surface as container rows so QML needs only one model.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerListRow {
    pub row_kind: ContainerRowKind,
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub section: ContainerSection,
    pub group_id: String,
    pub group_total_count: usize,
    pub group_running_count: usize,
    pub group_paused_count: usize,
    pub group_stopped_count: usize,
    pub depth: u8,
    pub expanded: bool,
    pub selected: bool,
    pub operation: String,
    pub health: String,
    pub ports: String,
}

impl ContainerListRow {
    pub fn section_header(section: ContainerSection, count: usize) -> Self {
        Self {
            row_kind: ContainerRowKind::SectionHeader,
            id: section.as_str().to_string(),
            name: section.title().to_string(),
            image: String::new(),
            state: section.as_str().to_string(),
            status: format!("{count}"),
            section,
            group_id: String::new(),
            group_total_count: count,
            group_running_count: 0,
            group_paused_count: 0,
            group_stopped_count: 0,
            depth: 0,
            expanded: false,
            selected: false,
            operation: String::new(),
            health: String::new(),
            ports: String::new(),
        }
    }

    pub fn group(
        group: &ContainerGroupSummary,
        group_id: String,
        expanded: bool,
        selected: bool,
        operation: String,
    ) -> Self {
        Self {
            row_kind: ContainerRowKind::Group,
            id: group_id.clone(),
            name: group.display_name.clone(),
            image: String::new(),
            state: ContainerSection::from_group(group).as_str().to_string(),
            status: format!("{} / {} running", group.running_count, group.total_count),
            section: ContainerSection::from_group(group),
            group_id,
            group_total_count: group.total_count,
            group_running_count: group.running_count,
            group_paused_count: group.paused_count,
            group_stopped_count: group.stopped_count,
            depth: 0,
            expanded,
            selected,
            operation,
            health: if group.unhealthy_count > 0 {
                format!("{} unhealthy", group.unhealthy_count)
            } else {
                String::new()
            },
            ports: String::new(),
        }
    }

    pub fn container(
        summary: &ContainerSummary,
        kind: ContainerRowKind,
        section: ContainerSection,
        group_id: String,
        selected: bool,
        operation: ContainerOperationState,
    ) -> Self {
        Self {
            row_kind: kind,
            id: summary.id.clone(),
            name: summary.display_name().to_string(),
            image: summary.image_name().to_string(),
            state: summary.state.as_str().to_string(),
            status: summary.status_text().to_string(),
            section,
            group_id,
            group_total_count: 0,
            group_running_count: 0,
            group_paused_count: 0,
            group_stopped_count: 0,
            depth: u8::from(kind == ContainerRowKind::ContainerChild),
            expanded: false,
            selected,
            operation: operation_name(operation).to_string(),
            health: health_name(summary).to_string(),
            ports: summary
                .ports
                .iter()
                .map(|port| port.display())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub fn selectable(&self) -> bool {
        self.row_kind != ContainerRowKind::SectionHeader
    }
}

pub fn operation_name(operation: ContainerOperationState) -> &'static str {
    match operation {
        ContainerOperationState::Idle => "",
        ContainerOperationState::Starting => "starting",
        ContainerOperationState::Stopping => "stopping",
        ContainerOperationState::Restarting => "restarting",
        ContainerOperationState::Killing => "killing",
        ContainerOperationState::Pausing => "pausing",
        ContainerOperationState::Unpausing => "unpausing",
        ContainerOperationState::Removing => "removing",
        ContainerOperationState::Renaming => "renaming",
    }
}

fn health_name(summary: &ContainerSummary) -> &'static str {
    match summary.health_summary().map(|health| health.status) {
        Some(ContainerHealthState::Starting) => "starting",
        Some(ContainerHealthState::Healthy) => "healthy",
        Some(ContainerHealthState::Unhealthy) => "unhealthy",
        Some(ContainerHealthState::Unknown) => "unknown",
        Some(ContainerHealthState::None) | None => "",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyViewRow {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortViewRow {
    pub container_port: String,
    pub protocol: String,
    pub host_ip: String,
    pub host_port: String,
    pub published: bool,
    pub browser_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountViewRow {
    pub mount_type: String,
    pub source: String,
    pub destination: String,
    pub access: String,
    pub propagation: String,
    pub volume_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkViewRow {
    pub name: String,
    pub id: String,
    pub ipv4: String,
    pub ipv6: String,
    pub gateway: String,
    pub mac: String,
    pub aliases: String,
    pub endpoint_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EnvironmentViewRow {
    pub key: String,
    value: String,
    pub revealed: bool,
}

impl fmt::Debug for EnvironmentViewRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentViewRow")
            .field("key", &self.key)
            .field("value", &"<redacted>")
            .field("revealed", &self.revealed)
            .finish()
    }
}

impl EnvironmentViewRow {
    pub fn masked_value(&self) -> &str {
        if self.revealed {
            &self.value
        } else {
            "••••••••"
        }
    }

    pub fn reveal(&mut self) {
        self.revealed = true;
    }

    pub fn conceal(&mut self) {
        self.revealed = false;
    }
}

/// Structured adapter for every Phase 3 Info section. This object contains no
/// raw inspect JSON and never serializes environment values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerDetailView {
    pub name: String,
    pub id: String,
    pub short_id: String,
    pub image: String,
    pub image_id: String,
    pub state_name: String,
    pub compose_project: String,
    pub general: Vec<PropertyViewRow>,
    pub state: Vec<PropertyViewRow>,
    pub health: Vec<PropertyViewRow>,
    pub ports: Vec<PortViewRow>,
    pub mounts: Vec<MountViewRow>,
    pub networks: Vec<NetworkViewRow>,
    pub configuration: Vec<PropertyViewRow>,
    pub environment: Vec<EnvironmentViewRow>,
    pub labels: Vec<PropertyViewRow>,
}

impl ContainerDetailView {
    pub fn from_detail(detail: &ContainerDetail) -> Self {
        Self::from_detail_for_endpoint(detail, "local")
    }

    pub fn from_detail_for_endpoint(detail: &ContainerDetail, endpoint_key: &str) -> Self {
        let summary = &detail.summary;
        let compose_project = summary
            .compose_metadata()
            .map(|metadata| metadata.project_name)
            .unwrap_or_default();
        let general = vec![
            property("Name", summary.display_name()),
            property("ID", &summary.id),
            property("Image", summary.image_name()),
            property("Image ID", &summary.image_id),
            property("Created", &format_time(summary.created_at_opt())),
            property("Platform", value(detail.platform.as_deref())),
            property("Hostname", value(detail.hostname.as_deref())),
            property("Domain Name", value(detail.domain_name.as_deref())),
            property("Working Directory", value(detail.working_dir.as_deref())),
            property("User", value(detail.user.as_deref())),
        ];
        let state = vec![
            property("Status", summary.status_text()),
            boolean("Running", detail.state_detail.running),
            boolean("Paused", detail.state_detail.paused),
            boolean("Restarting", detail.state_detail.restarting),
            boolean("OOM Killed", detail.state_detail.oom_killed),
            boolean("Dead", detail.state_detail.dead),
            property(
                "Exit Code",
                &detail
                    .state_detail
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "—".to_string()),
            ),
            property("Error", value(detail.state_detail.error.as_deref())),
            property("Started At", &format_time(detail.state_detail.started_at)),
            property("Finished At", &format_time(detail.state_detail.finished_at)),
            property(
                "Restart Count",
                &detail.state_detail.restart_count.to_string(),
            ),
        ];
        let health = detail
            .health
            .as_ref()
            .map(|health| {
                let last = health
                    .last_check
                    .or_else(|| health.log.iter().filter_map(|entry| entry.end).max());
                vec![
                    property("Status", &health.status),
                    property(
                        "Failing Streak",
                        &health
                            .failing_streak
                            .map(|count| count.to_string())
                            .unwrap_or_else(|| "—".to_string()),
                    ),
                    property("Last Check", &format_time(last)),
                    property(
                        "Recent Health Logs",
                        &health
                            .log
                            .iter()
                            .filter_map(|entry| entry.output.as_deref())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ]
            })
            .unwrap_or_default();
        let ports = summary
            .ports
            .iter()
            .map(|port| {
                let host_ip = port.host_ip.clone().unwrap_or_default();
                let host_port = port
                    .host_port
                    .map(|port| port.to_string())
                    .unwrap_or_default();
                PortViewRow {
                    container_port: port.container_port.to_string(),
                    protocol: port.protocol.clone(),
                    host_ip: host_ip.clone(),
                    host_port,
                    published: port.is_published(),
                    browser_url: browser_url_for_endpoint(
                        endpoint_key,
                        &host_ip,
                        port.host_port,
                        port.container_port,
                    ),
                }
            })
            .collect();
        let mounts = detail
            .mounts
            .iter()
            .map(|mount| MountViewRow {
                mount_type: mount.mount_type.clone(),
                source: mount.source.clone().unwrap_or_default(),
                destination: mount.destination.clone(),
                access: if mount.rw { "Read/Write" } else { "Read-only" }.to_string(),
                propagation: mount.propagation.clone().unwrap_or_default(),
                volume_name: if ContainerMountType::from_docker(&mount.mount_type)
                    == ContainerMountType::Volume
                {
                    mount.name.clone().unwrap_or_default()
                } else {
                    String::new()
                },
            })
            .collect();
        let networks = detail
            .networks
            .iter()
            .map(|network| NetworkViewRow {
                name: network.network_name.clone(),
                id: network.network_id.clone().unwrap_or_default(),
                ipv4: network.ipv4.clone().unwrap_or_default(),
                ipv6: network.ipv6.clone().unwrap_or_default(),
                gateway: network
                    .gateway
                    .clone()
                    .or_else(|| network.ipv6_gateway.clone())
                    .unwrap_or_default(),
                mac: network.mac.clone().unwrap_or_default(),
                aliases: network.aliases.join(", "),
                endpoint_id: network.endpoint_id.clone().unwrap_or_default(),
            })
            .collect();
        let configuration = vec![
            property("Entrypoint", &command(&detail.entrypoint)),
            property("Command", &command(&detail.command)),
            property("Working Directory", value(detail.working_dir.as_deref())),
            property("User", value(detail.user.as_deref())),
            property("Stop Signal", value(detail.stop_signal.as_deref())),
            property(
                "Stop Timeout",
                &detail
                    .stop_timeout_seconds
                    .map(|seconds| format!("{seconds} seconds"))
                    .unwrap_or_else(|| "—".to_string()),
            ),
            property(
                "Restart Policy",
                &match detail.restart_policy.maximum_retry_count {
                    Some(maximum) => format!("{} ({maximum})", detail.restart_policy.name),
                    None => detail.restart_policy.name.clone(),
                },
            ),
            boolean("Auto Remove", detail.auto_remove),
            boolean("TTY", detail.tty),
            boolean("Open Stdin", detail.open_stdin),
            boolean("Read-only RootFS", detail.read_only_rootfs),
            boolean("Privileged", detail.privileged),
        ];
        let mut environment = detail
            .environment
            .iter()
            .map(|variable| EnvironmentViewRow {
                key: variable.name.clone(),
                value: variable.value.clone().unwrap_or_default(),
                revealed: false,
            })
            .collect::<Vec<_>>();
        environment.sort_by_key(|row| row.key.to_ascii_lowercase());

        Self {
            name: summary.display_name().to_string(),
            id: summary.id.clone(),
            short_id: summary.short_id.clone(),
            image: summary.image_name().to_string(),
            image_id: summary.image_id.clone(),
            state_name: summary.state.as_str().to_string(),
            compose_project,
            general,
            state,
            health,
            ports,
            mounts,
            networks,
            configuration,
            environment,
            labels: sorted_pairs(&summary.labels),
        }
    }

    pub fn reveal_environment(&mut self, index: usize) -> bool {
        let Some(row) = self.environment.get_mut(index) else {
            return false;
        };
        if row.revealed {
            return false;
        }
        row.reveal();
        true
    }

    pub fn conceal_environment(&mut self, index: usize) -> bool {
        let Some(row) = self.environment.get_mut(index) else {
            return false;
        };
        if !row.revealed {
            return false;
        }
        row.conceal();
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMemberViewRow {
    pub id: String,
    pub name: String,
    pub service: String,
    pub state: String,
    pub image: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerGroupDetailView {
    pub id: String,
    pub project_name: String,
    pub status: String,
    pub working_directory: String,
    pub compose_files: String,
    pub compose_version: String,
    pub members: Vec<GroupMemberViewRow>,
    pub metadata: Vec<PropertyViewRow>,
}

impl ContainerGroupDetailView {
    pub fn from_group(
        id: String,
        group: &ContainerGroupSummary,
        summaries: &[ContainerSummary],
    ) -> Self {
        let by_id = summaries
            .iter()
            .map(|summary| (summary.id.as_str(), summary))
            .collect::<BTreeMap<_, _>>();
        let members = group
            .containers
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .map(|summary| GroupMemberViewRow {
                id: summary.id.clone(),
                name: summary.display_name().to_string(),
                service: summary
                    .compose_metadata()
                    .map(|metadata| metadata.service)
                    .unwrap_or_default(),
                state: summary.state.as_str().to_string(),
                image: summary.image_name().to_string(),
            })
            .collect();
        let status = format!("{} / {} running", group.running_count, group.total_count);
        Self {
            id,
            project_name: group.project_name.clone(),
            status: status.clone(),
            working_directory: group
                .working_directory
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            compose_files: group
                .config_files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            compose_version: group.compose_version.clone().unwrap_or_default(),
            members,
            metadata: vec![
                property("Project Name", &group.project_name),
                property("Status", &status),
                property("Containers", &group.total_count.to_string()),
                property("Unhealthy", &group.unhealthy_count.to_string()),
                property("Dev Containers", &group.devcontainer_count.to_string()),
            ],
        }
    }
}

fn property(key: &str, value: &str) -> PropertyViewRow {
    PropertyViewRow {
        key: key.to_string(),
        value: if value.is_empty() { "—" } else { value }.to_string(),
    }
}

fn boolean(key: &str, value: bool) -> PropertyViewRow {
    property(key, if value { "Yes" } else { "No" })
}

fn value(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("—")
}

fn format_time(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn command(values: &[String]) -> String {
    if values.is_empty() {
        "—".to_string()
    } else {
        values.join(" ")
    }
}

fn sorted_pairs(values: &BTreeMap<String, String>) -> Vec<PropertyViewRow> {
    values
        .iter()
        .map(|(key, value)| property(key, value))
        .collect()
}

fn browser_url_for_endpoint(
    endpoint_key: &str,
    host_ip: &str,
    host_port: Option<u16>,
    container_port: u16,
) -> String {
    let Some(port) = host_port else {
        return String::new();
    };
    let wildcard = matches!(host_ip, "" | "0.0.0.0" | "::" | "[::]");
    let host = match endpoint_browser_target(endpoint_key) {
        BrowserEndpoint::Local if wildcard => "localhost".to_string(),
        BrowserEndpoint::Local => host_ip.to_string(),
        // A published address belongs to the daemon host, not the machine
        // running the GUI. Always use the resolved daemon host remotely.
        BrowserEndpoint::Remote(host) => host,
        BrowserEndpoint::UnknownRemote => return String::new(),
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    let scheme = if container_port == 443 || port == 443 {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://{host}:{port}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserEndpoint {
    Local,
    Remote(String),
    UnknownRemote,
}

fn endpoint_browser_target(endpoint_key: &str) -> BrowserEndpoint {
    if endpoint_key == "local"
        || endpoint_key == "default-local"
        || endpoint_key.starts_with("unix://")
        || endpoint_key.starts_with("npipe://")
    {
        return BrowserEndpoint::Local;
    }

    let Some((scheme, remainder)) = endpoint_key.split_once("://") else {
        return BrowserEndpoint::UnknownRemote;
    };
    if !matches!(scheme, "tcp" | "http" | "https" | "ssh") {
        return BrowserEndpoint::UnknownRemote;
    }

    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    // Effective endpoint keys are already redacted by DockerClient, but strip
    // userinfo defensively so a legacy/raw key can never expose credentials.
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed
            .find(']')
            .map(|end| BrowserEndpoint::Remote(bracketed[..end].to_string()))
            .unwrap_or(BrowserEndpoint::UnknownRemote);
    }

    let host = if authority.matches(':').count() <= 1 {
        authority.split(':').next().unwrap_or_default()
    } else {
        // Unbracketed IPv6 has no unambiguous port boundary. Treat the whole
        // authority as the address; standards-compliant endpoint keys retain
        // brackets and take the branch above.
        authority
    };
    if host.is_empty() {
        BrowserEndpoint::UnknownRemote
    } else {
        BrowserEndpoint::Remote(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeZone;
    use tuxstack_docker_core::{
        COMPOSE_PROJECT_LABEL, ContainerStateDetail, EnvironmentVariable, MountInfo,
        NetworkAttachment, PortBinding, ResourceLimits, RestartPolicy, group_compose_containers,
    };

    use super::*;

    fn summary() -> ContainerSummary {
        ContainerSummary {
            id: "abcdef0123456789".into(),
            short_id: "abcdef012345".into(),
            name: "web".into(),
            image: "nginx:latest".into(),
            image_id: "sha256:image".into(),
            state: ContainerRuntimeState::Running,
            status: "Up 1 minute (healthy)".into(),
            created_at: Utc.timestamp_opt(100, 0).unwrap(),
            ports: vec![PortBinding {
                host_ip: Some("0.0.0.0".into()),
                host_port: Some(8080),
                container_port: 80,
                protocol: "tcp".into(),
            }],
            labels: BTreeMap::from([("team".into(), "platform".into())]),
        }
    }

    fn detail() -> ContainerDetail {
        ContainerDetail {
            summary: summary(),
            command: vec!["nginx".into(), "-g".into()],
            entrypoint: vec!["/entrypoint".into()],
            environment: vec![EnvironmentVariable {
                name: "TOKEN".into(),
                value: Some("top-secret".into()),
            }],
            mounts: vec![MountInfo {
                source: Some("named-data".into()),
                destination: "/data".into(),
                mode: None,
                rw: false,
                mount_type: "volume".into(),
                name: Some("named-data".into()),
                propagation: None,
            }],
            networks: vec![NetworkAttachment {
                network_name: "bridge".into(),
                network_id: Some("network-id".into()),
                ipv4: Some("172.17.0.2".into()),
                ipv6: None,
                gateway: Some("172.17.0.1".into()),
                ipv6_gateway: None,
                mac: Some("00:11:22:33:44:55".into()),
                aliases: vec!["web".into()],
                endpoint_id: Some("endpoint".into()),
            }],
            restart_policy: RestartPolicy {
                name: "unless-stopped".into(),
                maximum_retry_count: None,
            },
            health: None,
            platform: Some("linux".into()),
            hostname: Some("web-host".into()),
            domain_name: None,
            working_dir: Some("/app".into()),
            user: None,
            stop_signal: Some("SIGTERM".into()),
            stop_timeout_seconds: Some(10),
            auto_remove: false,
            tty: false,
            open_stdin: false,
            read_only_rootfs: false,
            privileged: false,
            state_detail: ContainerStateDetail {
                running: true,
                ..Default::default()
            },
            resource_limits: ResourceLimits::default(),
        }
    }

    #[test]
    fn list_row_contains_no_stats_and_maps_required_roles() {
        let row = ContainerListRow::container(
            &summary(),
            ContainerRowKind::Individual,
            ContainerSection::Running,
            String::new(),
            true,
            ContainerOperationState::Stopping,
        );
        assert_eq!(row.operation, "stopping");
        assert_eq!(row.health, "healthy");
        assert_eq!(row.ports, "0.0.0.0:8080->80/tcp");
        assert!(row.selected);
    }

    #[test]
    fn detail_is_structured_and_environment_is_masked_by_default() {
        let view = ContainerDetailView::from_detail(&detail());
        assert_eq!(view.general[0].key, "Name");
        assert_eq!(view.ports[0].browser_url, "http://localhost:8080");
        assert_eq!(view.mounts[0].access, "Read-only");
        assert_eq!(view.networks[0].name, "bridge");
        assert_eq!(view.environment[0].masked_value(), "••••••••");
    }

    #[test]
    fn environment_reveal_is_per_row_and_debug_is_redacted() {
        let mut view = ContainerDetailView::from_detail(&detail());
        assert!(view.reveal_environment(0));
        assert_eq!(view.environment[0].masked_value(), "top-secret");
        assert!(!view.reveal_environment(0));
        let debug = format!("{:?}", view.environment[0]);
        assert!(!debug.contains("top-secret"));
        assert!(view.conceal_environment(0));
        assert_eq!(view.environment[0].masked_value(), "••••••••");
    }

    #[test]
    fn unpublished_port_never_gets_browser_url() {
        assert_eq!(browser_url_for_endpoint("local", "", None, 80), "");
        assert_eq!(
            browser_url_for_endpoint("unix:///var/run/docker.sock", "::", Some(443), 443),
            "https://localhost:443"
        );
    }

    #[test]
    fn remote_browser_urls_use_redacted_endpoint_host_for_all_supported_syntaxes() {
        for endpoint in [
            "tcp://user:secret@docker.example:2375",
            "http://docker.example:2375",
            "https://docker.example:2376/path",
            "ssh://user@docker.example:22",
        ] {
            assert_eq!(
                browser_url_for_endpoint(endpoint, "0.0.0.0", Some(8080), 80),
                "http://docker.example:8080"
            );
        }
    }

    #[test]
    fn remote_browser_urls_never_substitute_local_published_addresses() {
        assert_eq!(
            browser_url_for_endpoint("tcp://docker.example:2375", "127.0.0.1", Some(8080), 80,),
            "http://docker.example:8080"
        );
        assert_eq!(
            browser_url_for_endpoint("opaque-remote-key", "0.0.0.0", Some(8080), 80),
            ""
        );
    }

    #[test]
    fn remote_browser_urls_preserve_ipv6_brackets_and_endpoint_isolation() {
        let first = browser_url_for_endpoint("tcp://[2001:db8::10]:2375", "::", Some(8443), 443);
        let second = browser_url_for_endpoint("tcp://[2001:db8::20]:2375", "::", Some(8443), 443);

        assert_eq!(first, "https://[2001:db8::10]:8443");
        assert_eq!(second, "https://[2001:db8::20]:8443");
        assert_ne!(first, second);
    }

    #[test]
    fn compose_group_ids_are_isolated_by_effective_endpoint() {
        let mut container = summary();
        container
            .labels
            .insert(COMPOSE_PROJECT_LABEL.into(), "project".into());
        let first = group_compose_containers("tcp://first.example:2375", &[container.clone()]);
        let second = group_compose_containers("tcp://second.example:2375", &[container]);

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].id, second[0].id);
    }
}
