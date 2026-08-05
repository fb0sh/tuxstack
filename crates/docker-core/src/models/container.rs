//! Container domain models.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use chrono::{DateTime, Utc};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::ComposeContainerMetadata;

/// A short list entry for a container.
///
/// The legacy field names remain because Images, Volumes and the current GUI
/// construct this type directly. The accessors expose the Container Core names
/// without maintaining a second summary type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerSummary {
    pub id: String,
    pub short_id: String,
    /// Compatibility alias for `display_name`.
    pub name: String,
    /// Compatibility alias for `image_name`.
    pub image: String,
    pub image_id: String,
    pub state: ContainerRuntimeState,
    /// Compatibility alias for `status_text`.
    pub status: String,
    /// Unknown list timestamps are represented by Unix epoch; use
    /// [`Self::created_at_opt`] to distinguish them.
    pub created_at: DateTime<Utc>,
    pub ports: Vec<ContainerPortSummary>,
    pub labels: BTreeMap<String, String>,
}

impl ContainerSummary {
    pub fn short_id(&self) -> &str {
        &self.short_id
    }

    pub fn names(&self) -> Vec<String> {
        if self.name.is_empty() {
            Vec::new()
        } else {
            vec![self.name.clone()]
        }
    }

    pub fn display_name(&self) -> &str {
        &self.name
    }

    pub fn image_name(&self) -> &str {
        &self.image
    }

    pub fn status_text(&self) -> &str {
        &self.status
    }

    /// A timestamp of Unix epoch is the stable compatibility sentinel for an
    /// unknown Docker list timestamp. It is never replaced with wall-clock now.
    pub fn created_at_opt(&self) -> Option<DateTime<Utc>> {
        (self.created_at.timestamp() != 0).then_some(self.created_at)
    }

    pub fn compose_metadata(&self) -> Option<ComposeContainerMetadata> {
        ComposeContainerMetadata::from_labels(&self.labels)
    }

    pub fn health_summary(&self) -> Option<ContainerHealthSummary> {
        let lower = self.status.to_ascii_lowercase();
        let status = if lower.contains("(unhealthy)") {
            ContainerHealthState::Unhealthy
        } else if lower.contains("(healthy)") {
            ContainerHealthState::Healthy
        } else if lower.contains("health: starting") || lower.contains("(health: starting)") {
            ContainerHealthState::Starting
        } else {
            return None;
        };
        Some(ContainerHealthSummary { status })
    }
}

/// Container lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerRuntimeState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

/// Compatibility name used by Images, Volumes and existing GUI code.
pub type ContainerState = ContainerRuntimeState;

impl ContainerRuntimeState {
    pub fn from_str_opt(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "created" => Self::Created,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "removing" => Self::Removing,
            "exited" | "stopped" => Self::Exited,
            "dead" => Self::Dead,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Restarting => "restarting",
            Self::Removing => "removing",
            Self::Exited => "exited",
            Self::Dead => "dead",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Restarting)
    }

    pub fn is_stopped(&self) -> bool {
        matches!(
            self,
            Self::Created | Self::Exited | Self::Dead | Self::Unknown
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerOperationState {
    #[default]
    Idle,
    Starting,
    Stopping,
    Restarting,
    Killing,
    Pausing,
    Unpausing,
    Removing,
    Renaming,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerSelection {
    #[default]
    None,
    Group {
        group_id: super::ContainerGroupId,
    },
    Container {
        container_id: String,
    },
}

/// A host-to-container port mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBinding {
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
    pub container_port: u16,
    pub protocol: String,
}

pub type ContainerPortSummary = PortBinding;

impl PortBinding {
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

    pub fn is_published(&self) -> bool {
        self.host_port.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerMountType {
    Volume,
    Bind,
    Tmpfs,
    NamedPipe,
    Cluster,
    Image,
    Unknown,
}

impl ContainerMountType {
    pub fn from_docker(value: &str) -> Self {
        match value {
            "volume" => Self::Volume,
            "bind" => Self::Bind,
            "tmpfs" => Self::Tmpfs,
            "npipe" => Self::NamedPipe,
            "cluster" => Self::Cluster,
            "image" => Self::Image,
            _ => Self::Unknown,
        }
    }

    pub fn as_docker(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Bind => "bind",
            Self::Tmpfs => "tmpfs",
            Self::NamedPipe => "npipe",
            Self::Cluster => "cluster",
            Self::Image => "image",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerHealthState {
    None,
    Starting,
    Healthy,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerHealthSummary {
    pub status: ContainerHealthState,
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
    pub domain_name: Option<String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub stop_signal: Option<String>,
    pub stop_timeout_seconds: Option<i64>,
    pub auto_remove: bool,
    pub tty: bool,
    pub open_stdin: bool,
    pub read_only_rootfs: bool,
    pub privileged: bool,
    pub state_detail: ContainerStateDetail,
    pub resource_limits: ResourceLimits,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: Option<String>,
}

impl fmt::Debug for EnvironmentVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentVariable")
            .field("name", &self.name)
            .field("value", &self.value.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl EnvironmentVariable {
    pub fn display(&self) -> String {
        match &self.value {
            Some(v) => format!("{}={}", self.name, v),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerMountSummary {
    pub source: Option<String>,
    pub destination: String,
    pub mode: Option<String>,
    pub rw: bool,
    pub mount_type: String,
    pub name: Option<String>,
    pub propagation: Option<String>,
}

/// Compatibility name used by the existing detail GUI.
pub type MountInfo = ContainerMountSummary;

impl ContainerMountSummary {
    pub fn read_only(&self) -> bool {
        !self.rw
    }

    pub fn typed_mount_type(&self) -> ContainerMountType {
        ContainerMountType::from_docker(&self.mount_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNetworkSummary {
    pub network_name: String,
    pub network_id: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub gateway: Option<String>,
    pub ipv6_gateway: Option<String>,
    pub mac: Option<String>,
    pub aliases: Vec<String>,
    pub endpoint_id: Option<String>,
}

/// Compatibility names for the single network attachment domain type.
pub type NetworkAttachment = ContainerNetworkSummary;
pub type ContainerNetworkDetail = ContainerNetworkSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub name: String,
    pub maximum_retry_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub failing_streak: Option<u64>,
    pub last_check: Option<DateTime<Utc>>,
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
pub struct ContainerStateDetail {
    pub running: bool,
    pub paused: bool,
    pub restarting: bool,
    pub oom_killed: bool,
    pub dead: bool,
    pub exit_code: Option<i64>,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub restart_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory_bytes: Option<u64>,
    pub nano_cpus: Option<u64>,
    pub pids_limit: Option<i64>,
    pub cpu_shares: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerSortMode {
    NameAscending,
    NameDescending,
    NewestFirst,
    OldestFirst,
    RunningFirst,
    StoppedFirst,
    ComposeGroupsFirst,
    IndividualContainersFirst,
}

/// Pure, stable container sorting. Equal keys are resolved by full ID.
pub fn sort_containers(containers: &mut [ContainerSummary], mode: ContainerSortMode) {
    containers.sort_by(|a, b| {
        let name = || {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        };
        match mode {
            ContainerSortMode::NameAscending => name(),
            ContainerSortMode::NameDescending => b
                .name
                .to_ascii_lowercase()
                .cmp(&a.name.to_ascii_lowercase())
                .then_with(|| a.id.cmp(&b.id)),
            ContainerSortMode::NewestFirst => b.created_at.cmp(&a.created_at).then_with(name),
            ContainerSortMode::OldestFirst => a.created_at.cmp(&b.created_at).then_with(name),
            ContainerSortMode::RunningFirst => state_sort_key(a.state)
                .cmp(&state_sort_key(b.state))
                .then_with(name),
            ContainerSortMode::StoppedFirst => state_sort_key(b.state)
                .cmp(&state_sort_key(a.state))
                .then_with(name),
            ContainerSortMode::ComposeGroupsFirst => b
                .compose_metadata()
                .is_some()
                .cmp(&a.compose_metadata().is_some())
                .then_with(name),
            ContainerSortMode::IndividualContainersFirst => a
                .compose_metadata()
                .is_some()
                .cmp(&b.compose_metadata().is_some())
                .then_with(name),
        }
    });
}

fn state_sort_key(state: ContainerRuntimeState) -> u8 {
    match state {
        ContainerRuntimeState::Restarting => 0,
        ContainerRuntimeState::Running => 1,
        ContainerRuntimeState::Paused => 2,
        _ => 3,
    }
}

/// Case-insensitive local search over every field available in a list response.
pub fn container_matches_search(container: &ContainerSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let compose = container.compose_metadata();
    [
        container.name.as_str(),
        container.id.as_str(),
        container.short_id.as_str(),
        container.image.as_str(),
        container.image_id.as_str(),
        container.state.as_str(),
        compose
            .as_ref()
            .map(|c| c.project_name.as_str())
            .unwrap_or(""),
        compose.as_ref().map(|c| c.service.as_str()).unwrap_or(""),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(&query))
        || container
            .ports
            .iter()
            .any(|port| port.display().to_ascii_lowercase().contains(&query))
        || container.labels.iter().any(|(key, value)| {
            key.to_ascii_lowercase().contains(&query) || value.to_ascii_lowercase().contains(&query)
        })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartContainerOptions {
    pub timeout_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillContainerOptions {
    pub signal: String,
}

impl Default for KillContainerOptions {
    fn default() -> Self {
        Self {
            signal: "SIGKILL".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerPortProtocol {
    Tcp,
    Udp,
    Sctp,
}

impl ContainerPortProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sctp => "sctp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerPort {
    pub container_port: u16,
    pub protocol: ContainerPortProtocol,
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreateContainerMount {
    Volume {
        source: String,
        destination: String,
        read_only: bool,
    },
    Bind {
        source: String,
        destination: String,
        read_only: bool,
        propagation: Option<String>,
    },
    Tmpfs {
        destination: String,
        size_bytes: Option<u64>,
        mode: Option<u32>,
    },
}

impl CreateContainerMount {
    pub fn destination(&self) -> &str {
        match self {
            Self::Volume { destination, .. }
            | Self::Bind { destination, .. }
            | Self::Tmpfs { destination, .. } => destination,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEnvironmentVariable {
    pub key: String,
    pub value: String,
}

impl fmt::Debug for CreateEnvironmentVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateEnvironmentVariable")
            .field("key", &self.key)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerNetwork {
    pub name: String,
    pub aliases: Vec<String>,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerRestartPolicyName {
    No,
    Always,
    UnlessStopped,
    OnFailure,
}

impl ContainerRestartPolicyName {
    pub fn as_docker(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::Always => "always",
            Self::UnlessStopped => "unless-stopped",
            Self::OnFailure => "on-failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerRestartPolicy {
    pub name: ContainerRestartPolicyName,
    pub maximum_retry_count: Option<u64>,
}

impl Default for ContainerRestartPolicy {
    fn default() -> Self {
        Self {
            name: ContainerRestartPolicyName::No,
            maximum_retry_count: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerResources {
    pub cpu_cores_millis: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids_limit: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerRequest {
    pub name: Option<String>,
    pub image: String,
    pub platform: Option<String>,
    pub hostname: Option<String>,
    pub domain_name: Option<String>,
    pub entrypoint: Vec<String>,
    pub command: Vec<String>,
    pub working_directory: Option<String>,
    pub user: Option<String>,
    pub tty: bool,
    pub open_stdin: bool,
    pub ports: Vec<CreateContainerPort>,
    pub mounts: Vec<CreateContainerMount>,
    pub environment: Vec<CreateEnvironmentVariable>,
    pub networks: Vec<CreateContainerNetwork>,
    pub resources: CreateContainerResources,
    pub restart_policy: ContainerRestartPolicy,
    #[serde(deserialize_with = "deserialize_unique_labels")]
    pub labels: BTreeMap<String, String>,
    pub read_only_rootfs: bool,
    pub privileged: bool,
    pub auto_remove: bool,
    pub create_and_start: bool,
}

fn deserialize_unique_labels<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UniqueLabelsVisitor;

    impl<'de> Visitor<'de> for UniqueLabelsVisitor {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with unique container label keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut labels = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if labels.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate container label key: {key}"
                    )));
                }
            }
            Ok(labels)
        }
    }

    deserializer.deserialize_map(UniqueLabelsVisitor)
}

impl CreateContainerRequest {
    pub fn validate(&self) -> Result<(), CreateContainerValidationError> {
        if self.image.trim().is_empty() {
            return Err(CreateContainerValidationError::MissingImage);
        }
        if let Some(name) = self.name.as_deref() {
            if !valid_container_name(name) {
                return Err(CreateContainerValidationError::InvalidName(
                    name.to_string(),
                ));
            }
        }
        let mut bindings = BTreeSet::new();
        for port in &self.ports {
            if port.container_port == 0 {
                return Err(CreateContainerValidationError::InvalidContainerPort);
            }
            if port
                .host_ip
                .as_deref()
                .is_some_and(|address| address.parse::<IpAddr>().is_err())
            {
                return Err(CreateContainerValidationError::InvalidHostIp);
            }
            if port.host_port == Some(0) {
                return Err(CreateContainerValidationError::InvalidHostPort);
            }
            if let Some(host_port) = port.host_port {
                let key = (
                    port.host_ip
                        .as_deref()
                        .map(str::parse::<IpAddr>)
                        .transpose()
                        .map_err(|_| CreateContainerValidationError::InvalidHostIp)?,
                    host_port,
                    port.protocol.as_str(),
                );
                if !bindings.insert(key) {
                    return Err(CreateContainerValidationError::DuplicatePortBinding);
                }
            }
        }
        if self
            .working_directory
            .as_deref()
            .is_some_and(|path| !valid_container_path(path))
        {
            return Err(CreateContainerValidationError::InvalidWorkingDirectory);
        }
        let mut destinations = BTreeSet::new();
        for mount in &self.mounts {
            let destination = mount.destination();
            if !valid_container_path(destination) {
                return Err(CreateContainerValidationError::InvalidMountDestination(
                    destination.to_string(),
                ));
            }
            match mount {
                CreateContainerMount::Volume { source, .. } if source.trim().is_empty() => {
                    return Err(CreateContainerValidationError::InvalidMountSource);
                }
                CreateContainerMount::Bind { source, .. } if !valid_container_path(source) => {
                    return Err(CreateContainerValidationError::InvalidMountSource);
                }
                CreateContainerMount::Tmpfs {
                    size_bytes: Some(0),
                    ..
                } => return Err(CreateContainerValidationError::InvalidTmpfsSize),
                CreateContainerMount::Tmpfs {
                    mode: Some(mode), ..
                } if *mode > 0o7777 => {
                    return Err(CreateContainerValidationError::InvalidTmpfsMode);
                }
                _ => {}
            }
            if !destinations.insert(destination) {
                return Err(CreateContainerValidationError::DuplicateMountDestination(
                    destination.to_string(),
                ));
            }
        }
        let mut env_keys = BTreeSet::new();
        for variable in &self.environment {
            if variable.key.is_empty()
                || variable.key.contains('=')
                || variable.key.as_bytes().contains(&0)
                || variable.value.as_bytes().contains(&0)
                || !env_keys.insert(variable.key.as_str())
            {
                return Err(
                    CreateContainerValidationError::InvalidOrDuplicateEnvironmentKey(
                        variable.key.clone(),
                    ),
                );
            }
        }
        let mut networks = BTreeSet::new();
        for network in &self.networks {
            let mut aliases = BTreeSet::new();
            if network.name.trim().is_empty()
                || network.name.as_bytes().contains(&0)
                || network.aliases.iter().any(|alias| {
                    alias.trim().is_empty()
                        || alias.as_bytes().contains(&0)
                        || !aliases.insert(alias.as_str())
                })
                || network
                    .ipv4_address
                    .as_deref()
                    .is_some_and(|address| address.parse::<Ipv4Addr>().is_err())
                || network
                    .ipv6_address
                    .as_deref()
                    .is_some_and(|address| address.parse::<Ipv6Addr>().is_err())
                || !networks.insert(network.name.as_str())
            {
                return Err(CreateContainerValidationError::InvalidOrDuplicateNetwork(
                    network.name.clone(),
                ));
            }
        }
        if self.resources.cpu_cores_millis == Some(0) {
            return Err(CreateContainerValidationError::InvalidCpuLimit);
        }
        if self.resources.memory_bytes == Some(0) {
            return Err(CreateContainerValidationError::InvalidMemoryLimit);
        }
        if self.resources.pids_limit.is_some_and(|limit| limit <= 0) {
            return Err(CreateContainerValidationError::InvalidPidsLimit);
        }
        if (self.restart_policy.name != ContainerRestartPolicyName::OnFailure
            && self.restart_policy.maximum_retry_count.is_some())
            || (self.auto_remove && self.restart_policy.name != ContainerRestartPolicyName::No)
        {
            return Err(CreateContainerValidationError::InvalidRestartPolicy);
        }
        if self.labels.iter().any(|(key, value)| {
            key.trim().is_empty() || key.as_bytes().contains(&0) || value.as_bytes().contains(&0)
        }) || self
            .command
            .iter()
            .chain(&self.entrypoint)
            .any(|value| value.as_bytes().contains(&0))
        {
            return Err(CreateContainerValidationError::InvalidTextValue);
        }
        Ok(())
    }
}

fn valid_container_path(path: &str) -> bool {
    if path == "/" {
        return true;
    }
    path.starts_with('/')
        && !path.as_bytes().contains(&0)
        && path
            .split('/')
            .skip(1)
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn valid_container_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CreateContainerValidationError {
    #[error("an image reference is required")]
    MissingImage,
    #[error("invalid container name: {0}")]
    InvalidName(String),
    #[error("container port must be between 1 and 65535")]
    InvalidContainerPort,
    #[error("host port must be between 1 and 65535")]
    InvalidHostPort,
    #[error("published host IP address is invalid")]
    InvalidHostIp,
    #[error("duplicate published host port binding")]
    DuplicatePortBinding,
    #[error("mount destination must be an absolute container path: {0}")]
    InvalidMountDestination(String),
    #[error("duplicate mount destination: {0}")]
    DuplicateMountDestination(String),
    #[error("mount source is invalid")]
    InvalidMountSource,
    #[error("tmpfs size must be greater than zero")]
    InvalidTmpfsSize,
    #[error("tmpfs mode must be between 0 and 07777")]
    InvalidTmpfsMode,
    #[error("working directory must be an absolute container path")]
    InvalidWorkingDirectory,
    #[error("a command, entrypoint, label, or environment value is invalid")]
    InvalidTextValue,
    #[error("invalid or duplicate environment key: {0}")]
    InvalidOrDuplicateEnvironmentKey(String),
    #[error("invalid or duplicate network: {0}")]
    InvalidOrDuplicateNetwork(String),
    #[error("CPU limit must be greater than zero")]
    InvalidCpuLimit,
    #[error("memory limit must be greater than zero")]
    InvalidMemoryLimit,
    #[error("PIDs limit must be greater than zero")]
    InvalidPidsLimit,
    #[error("maximum retry count is only valid for on-failure restart policy")]
    InvalidRestartPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNetworkFailure {
    pub network: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateContainerResult {
    pub id: String,
    pub warnings: Vec<String>,
    pub network_failures: Vec<ContainerNetworkFailure>,
    pub started: bool,
    pub start_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    Console,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: Option<DateTime<Utc>>,
    pub stream: LogStream,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn summary(name: &str, state: ContainerRuntimeState, created: i64) -> ContainerSummary {
        ContainerSummary {
            id: format!("{name}-full-id"),
            short_id: format!("{name}-short"),
            name: name.to_string(),
            image: "example/web:latest".to_string(),
            image_id: "sha256:image".to_string(),
            state,
            status: state.as_str().to_string(),
            created_at: Utc.timestamp_opt(created, 0).unwrap(),
            ports: vec![],
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn runtime_states_are_total_and_active_is_precise() {
        for (raw, expected) in [
            ("created", ContainerRuntimeState::Created),
            ("running", ContainerRuntimeState::Running),
            ("paused", ContainerRuntimeState::Paused),
            ("restarting", ContainerRuntimeState::Restarting),
            ("removing", ContainerRuntimeState::Removing),
            ("exited", ContainerRuntimeState::Exited),
            ("dead", ContainerRuntimeState::Dead),
            ("future", ContainerRuntimeState::Unknown),
        ] {
            assert_eq!(ContainerRuntimeState::from_str_opt(raw), expected);
        }
        assert!(ContainerRuntimeState::Running.is_active());
        assert!(!ContainerRuntimeState::Exited.is_active());
    }

    #[test]
    fn unknown_timestamp_sentinel_is_not_reported_as_known() {
        let unknown = summary("unknown", ContainerRuntimeState::Exited, 0);
        assert_eq!(unknown.created_at_opt(), None);
    }

    #[test]
    fn all_eight_sort_modes_are_stable() {
        let mut values = vec![
            summary("beta", ContainerRuntimeState::Exited, 20),
            summary("alpha", ContainerRuntimeState::Running, 10),
        ];
        for mode in [
            ContainerSortMode::NameAscending,
            ContainerSortMode::NameDescending,
            ContainerSortMode::NewestFirst,
            ContainerSortMode::OldestFirst,
            ContainerSortMode::RunningFirst,
            ContainerSortMode::StoppedFirst,
            ContainerSortMode::ComposeGroupsFirst,
            ContainerSortMode::IndividualContainersFirst,
        ] {
            sort_containers(&mut values, mode);
            assert_eq!(values.len(), 2);
        }
        sort_containers(&mut values, ContainerSortMode::RunningFirst);
        assert_eq!(values[0].name, "alpha");
        sort_containers(&mut values, ContainerSortMode::StoppedFirst);
        assert_eq!(values[0].name, "beta");
    }

    #[test]
    fn comprehensive_search_covers_ports_and_labels() {
        let mut value = summary("web", ContainerRuntimeState::Running, 10);
        value.ports.push(PortBinding {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(8080),
            container_port: 80,
            protocol: "tcp".into(),
        });
        value.labels.insert("team".into(), "platform".into());
        for query in [
            "WEB",
            "full-id",
            "short",
            "example/web",
            "running",
            "8080",
            "team",
            "platform",
        ] {
            assert!(container_matches_search(&value, query), "query {query}");
        }
        assert!(!container_matches_search(&value, "missing"));

        value
            .labels
            .insert(crate::models::COMPOSE_PROJECT_LABEL.into(), "demo".into());
        value.labels.insert(
            crate::models::COMPOSE_SERVICE_LABEL.into(),
            "frontend".into(),
        );
        assert!(container_matches_search(&value, "DEMO"));
        assert!(container_matches_search(&value, "front"));
    }

    #[test]
    fn environment_debug_redacts_values() {
        let value = EnvironmentVariable {
            name: "PASSWORD".into(),
            value: Some("secret".into()),
        };
        let output = format!("{value:?}");
        assert!(output.contains("PASSWORD"));
        assert!(!output.contains("secret"));
    }

    #[test]
    fn create_validation_rejects_duplicates_and_bad_limits() {
        let mut request = CreateContainerRequest {
            image: "busybox:latest".into(),
            name: Some("valid-name".into()),
            ..Default::default()
        };
        assert!(request.validate().is_ok());
        request.ports = vec![
            CreateContainerPort {
                container_port: 80,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: None,
                host_port: Some(8080),
            },
            CreateContainerPort {
                container_port: 81,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: None,
                host_port: Some(8080),
            },
        ];
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::DuplicatePortBinding)
        );
        request.ports.clear();
        request.resources.cpu_cores_millis = Some(0);
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidCpuLimit)
        );
    }

    #[test]
    fn create_validation_rejects_zero_host_port_tmpfs_size_and_pids() {
        let mut request = CreateContainerRequest {
            image: "busybox:latest".into(),
            ..Default::default()
        };
        request.ports.push(CreateContainerPort {
            container_port: 80,
            protocol: ContainerPortProtocol::Tcp,
            host_ip: None,
            host_port: Some(0),
        });
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidHostPort)
        );

        request.ports.clear();
        request.mounts.push(CreateContainerMount::Tmpfs {
            destination: "/run".into(),
            size_bytes: Some(0),
            mode: None,
        });
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidTmpfsSize)
        );

        request.mounts.clear();
        request.resources.pids_limit = Some(-1);
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidPidsLimit)
        );
    }

    #[test]
    fn create_validation_rejects_duplicate_network_aliases_and_canonical_bindings() {
        let mut request = CreateContainerRequest {
            image: "busybox:latest".into(),
            networks: vec![CreateContainerNetwork {
                name: "front".into(),
                aliases: vec!["web".into(), "web".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidOrDuplicateNetwork(
                "front".into()
            ))
        );

        request.networks.clear();
        request.ports = vec![
            CreateContainerPort {
                container_port: 80,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: Some("0:0:0:0:0:0:0:1".into()),
                host_port: Some(8080),
            },
            CreateContainerPort {
                container_port: 81,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: Some("::1".into()),
                host_port: Some(8080),
            },
        ];
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::DuplicatePortBinding)
        );
    }

    #[test]
    fn create_label_deserializer_rejects_duplicate_keys() {
        #[derive(Deserialize)]
        struct LabelsOnly {
            #[serde(deserialize_with = "deserialize_unique_labels")]
            labels: BTreeMap<String, String>,
        }

        let parsed = serde_json::from_str::<LabelsOnly>(
            r#"{"labels":{"team":"platform","team":"runtime"}}"#,
        );
        let error = parsed.err().expect("duplicate label key must fail");
        assert!(
            error
                .to_string()
                .contains("duplicate container label key: team")
        );

        let parsed = serde_json::from_str::<LabelsOnly>(r#"{"labels":{"team":"platform"}}"#)
            .expect("unique labels must parse");
        assert_eq!(
            parsed.labels.get("team").map(String::as_str),
            Some("platform")
        );
    }

    #[test]
    fn create_validation_rejects_unsafe_paths_networks_and_policy_conflicts() {
        let mut request = CreateContainerRequest {
            image: "busybox:latest".into(),
            working_directory: Some("relative".into()),
            ..Default::default()
        };
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidWorkingDirectory)
        );

        request.working_directory = None;
        request.networks.push(CreateContainerNetwork {
            name: "front".into(),
            ipv4_address: Some("not-an-ip".into()),
            ..Default::default()
        });
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidOrDuplicateNetwork(
                "front".into()
            ))
        );

        request.networks.clear();
        request.auto_remove = true;
        request.restart_policy.name = ContainerRestartPolicyName::Always;
        assert_eq!(
            request.validate(),
            Err(CreateContainerValidationError::InvalidRestartPolicy)
        );
    }

    #[test]
    fn action_option_defaults_are_real_docker_defaults() {
        assert_eq!(RestartContainerOptions::default().timeout_seconds, None);
        assert_eq!(KillContainerOptions::default().signal, "SIGKILL");
    }
}
