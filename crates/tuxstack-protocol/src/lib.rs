//! Typed, domain-neutral local IPC messages and bounded CBOR framing.
//!
//! The protocol deliberately contains no daemon, Docker API, Tokio, filesystem
//! I/O, or UI implementation. CBOR is used because its self-describing map and
//! enum representation permits additive schema evolution without introducing
//! untyped JSON values or method strings. File contents never belong in these
//! messages; clients receive FUSE paths and descriptors instead.

use std::fmt;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tuxstack_domain::{
    CloneVolumeRequest, ContainerDetail, ContainerGroupId, ContainerGroupOperationResult,
    ContainerGroupSummary, ContainerLogsOptions, ContainerRuntimeState, ContainerStats,
    ContainerSummary, CreateContainerRequest, CreateContainerResult, CreateNetworkOptions,
    CreateNetworkResult, CreateVolumeRequest, DockerSystemInfo, ImageDeleteResult, ImageDetail,
    ImagePullProgress, ImageSummary, KillContainerOptions, LogLine, NetworkDetail, NetworkSummary,
    OverviewData, PruneVolumeFilters, PullImageOptions, RemoveContainerOptions, RemoveImageOptions,
    RemoveVolumeOptions, RestartContainerOptions, StopContainerOptions, VolumeDetail,
    VolumePruneResult, VolumeSummary,
};

/// Current wire protocol version. Incompatible changes require a new value.
pub const PROTOCOL_VERSION: u32 = 1;
/// Absolute upper bound accepted by every decoder (32 MiB).
pub const MAX_FRAME_SIZE: u32 = 32 * 1024 * 1024;
/// Number of bytes in the unsigned big-endian frame length prefix.
pub const FRAME_HEADER_SIZE: usize = 4;

pub type RequestId = u64;
pub type SubscriptionId = u64;

/// An opaque, serializable feature bitmap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureFlags(pub u64);

impl FeatureFlags {
    pub const NONE: Self = Self(0);
    pub const SUBSCRIPTIONS: Self = Self(1 << 0);
    pub const REQUEST_CANCELLATION: Self = Self(1 << 1);
    pub const RESOURCE_DESCRIPTORS: Self = Self(1 << 2);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Read-only provider capabilities. There are intentionally no write bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderCapabilities(pub u32);

impl ProviderCapabilities {
    pub const NONE: Self = Self(0);
    pub const LOOKUP: Self = Self(1 << 0);
    pub const READDIR: Self = Self(1 << 1);
    pub const GETATTR: Self = Self(1 << 2);
    pub const READLINK: Self = Self(1 << 3);
    pub const OPEN: Self = Self(1 << 4);
    pub const READ: Self = Self(1 << 5);
    pub const DOWNLOAD: Self = Self(1 << 6);
    pub const REFRESH: Self = Self(1 << 7);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolEnvelope {
    pub protocol_version: u32,
    pub request_id: RequestId,
    pub body: ProtocolBody,
}

impl ProtocolEnvelope {
    #[must_use]
    pub const fn new(request_id: RequestId, body: ProtocolBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProtocolBody {
    Hello(ClientHello),
    Accepted(ServerHello),
    Rejected(HandshakeRejection),
    Request(Box<Request>),
    Response(Box<Response>),
    Subscribe(SubscriptionRequest),
    Subscribed(SubscriptionAccepted),
    Unsubscribe { subscription_id: SubscriptionId },
    Event(ServerEvent),
    CancelRequest { request_id: RequestId },
    Ping,
    Pong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub client_version: String,
    pub supported_protocol_versions: Vec<u32>,
    pub feature_flags: FeatureFlags,
    pub max_frame_size: u32,
}

impl ClientHello {
    #[must_use]
    pub fn current(client_version: impl Into<String>) -> Self {
        Self {
            client_version: client_version.into(),
            supported_protocol_versions: vec![PROTOCOL_VERSION],
            feature_flags: FeatureFlags::SUBSCRIPTIONS
                .union(FeatureFlags::REQUEST_CANCELLATION)
                .union(FeatureFlags::RESOURCE_DESCRIPTORS),
            max_frame_size: MAX_FRAME_SIZE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerHello {
    pub daemon_version: String,
    pub negotiated_protocol_version: u32,
    pub feature_flags: FeatureFlags,
    pub max_frame_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeRejection {
    pub code: HandshakeRejectionCode,
    pub message: String,
    pub daemon_version: String,
    pub supported_protocol_versions: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HandshakeRejectionCode {
    UnsupportedProtocol,
    InvalidMaximumFrameSize,
    MissingRequiredFeature,
    Unauthorized,
    ServerUnavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    GetDaemonStatus,
    GetMountStatus,
    SetMountState(MountAction),
    GetResourceFusePath(DockerResourceRef),
    GetProviderDescriptor(ResourcePath),
    PerformResourceOperation(ResourceOperation),
    Docker(Box<DockerRequest>),
    ContainerTerminalInput {
        subscription_id: SubscriptionId,
        bytes: Vec<u8>,
    },
    ContainerTerminalResize {
        subscription_id: SubscriptionId,
        rows: u16,
        cols: u16,
    },
    ContainerTerminalClose {
        subscription_id: SubscriptionId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Response {
    DaemonStatus(DaemonStatus),
    MountStatus(MountStatus),
    ResourceFusePath(ResourceFusePath),
    ProviderDescriptor(ProviderDescriptor),
    ResourceOperation(ResourceOperationResult),
    Docker(Box<DockerResponse>),
    Acknowledged,
    Error(ProtocolError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DockerRequest {
    SystemInfo,
    Overview,
    ListContainers {
        all: bool,
        limit: Option<usize>,
        search: Option<String>,
        state: Option<ContainerRuntimeState>,
    },
    InspectContainer {
        id: String,
    },
    StartContainer {
        id: String,
    },
    StopContainer {
        id: String,
        options: StopContainerOptions,
    },
    RestartContainer {
        id: String,
        options: RestartContainerOptions,
    },
    PauseContainer {
        id: String,
    },
    UnpauseContainer {
        id: String,
    },
    KillContainer {
        id: String,
        options: KillContainerOptions,
    },
    RemoveContainer {
        id: String,
        options: RemoveContainerOptions,
    },
    RenameContainer {
        id: String,
        new_name: String,
    },
    CreateContainer {
        request: Box<CreateContainerRequest>,
    },
    ContainerStats {
        id: String,
    },
    ContainerLogs {
        id: String,
        options: ContainerLogsOptions,
    },
    ListComposeProjects,
    ExecuteComposeTargets {
        group_id: ContainerGroupId,
        target_ids: Vec<String>,
        action: ComposeAction,
    },
    ListImages {
        search: Option<String>,
    },
    InspectImage {
        id: String,
    },
    RemoveImage {
        id: String,
        options: RemoveImageOptions,
    },
    PullImage {
        options: PullImageOptions,
    },
    ListNetworks {
        search: Option<String>,
    },
    InspectNetwork {
        id: String,
    },
    CreateNetwork {
        options: CreateNetworkOptions,
    },
    RemoveNetwork {
        id: String,
    },
    ListVolumes {
        search: Option<String>,
    },
    InspectVolume {
        name: String,
    },
    CreateVolume {
        request: CreateVolumeRequest,
    },
    RemoveVolume {
        name: String,
        options: RemoveVolumeOptions,
    },
    PruneVolumes {
        filters: PruneVolumeFilters,
    },
    CloneVolume {
        request: CloneVolumeRequest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComposeAction {
    Start,
    Stop(StopContainerOptions),
    Restart(RestartContainerOptions),
    Kill(KillContainerOptions),
    Pause,
    Unpause,
    Remove(RemoveContainerOptions),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DockerResponse {
    SystemInfo(DockerSystemInfo),
    Overview(OverviewData),
    Containers(Vec<ContainerSummary>),
    ContainerDetail(ContainerDetail),
    ContainerCreated(CreateContainerResult),
    ContainerStats(ContainerStats),
    ContainerLogs(Vec<LogLine>),
    ComposeProjects(Vec<ContainerGroupSummary>),
    ComposeOperation(ContainerGroupOperationResult),
    Images(Vec<ImageSummary>),
    ImageDetail(ImageDetail),
    ImagesRemoved(Vec<ImageDeleteResult>),
    ImagePullAccepted { subscription_id: SubscriptionId },
    Networks(Vec<NetworkSummary>),
    NetworkDetail(NetworkDetail),
    NetworkCreated(CreateNetworkResult),
    Volumes(Vec<VolumeSummary>),
    VolumeDetail(VolumeDetail),
    VolumesPruned(VolumePruneResult),
    Acknowledged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MountAction {
    Mount,
    Unmount,
    Remount,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DockerResourceRef {
    Container { container_id: String },
    Image { image_id: String },
    Volume { volume_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourcePath {
    pub resource: DockerResourceRef,
    /// Logical path components below the resource root. Implementations must
    /// validate each component and must not treat this as a host path.
    pub components: Vec<String>,
}

impl ResourcePath {
    #[must_use]
    pub const fn root(resource: DockerResourceRef) -> Self {
        Self {
            resource,
            components: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProviderKind {
    ContainerRootfsSnapshot,
    ContainerArchiveLive,
    NamedVolumeLive,
    LocalBindLive,
    HelperBindLive,
    TmpfsLive,
    RuntimeMount,
    ImageRootfsImmutable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConsistencyMode {
    Immutable,
    Live,
    Snapshot {
        captured_at_unix_ms: i64,
        generation: u64,
    },
    OperationTimeRead,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub kind: ProviderKind,
    pub consistency: ConsistencyMode,
    pub source: Option<String>,
    pub capabilities: ProviderCapabilities,
    pub status: ProviderStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProviderStatus {
    Ready,
    IndexBuilding { progress_percent: Option<u8> },
    SnapshotBuilding { progress_percent: Option<u8> },
    Unavailable { reason: String },
    PermissionDenied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceFusePath {
    pub resource: DockerResourceRef,
    /// Absolute, daemon-validated path in the mounted FUSE namespace.
    pub path: PathBuf,
    pub descriptor: ProviderDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub daemon_version: String,
    pub lifecycle: DaemonLifecycle,
    pub docker: DockerConnectionStatus,
    pub mount: MountStatus,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DaemonLifecycle {
    Starting,
    Ready,
    Stopping,
    Degraded { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DockerConnectionStatus {
    Connected { daemon_id: Option<String> },
    Reconnecting,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountStatus {
    pub state: MountState,
    pub mount_point: Option<PathBuf>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MountState {
    Unmounted,
    Mounting,
    Mounted,
    Unmounting,
    Failed { reason: String },
}

/// Typed operation categories that can gain variants without method strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResourceOperation {
    Refresh { path: ResourcePath },
    RebuildIndex { resource: DockerResourceRef },
    InvalidateCache { resource: DockerResourceRef },
    OpenResource { resource: DockerResourceRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResourceOperationResult {
    Completed,
    Accepted { operation_id: u64 },
    AlreadyCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ProtocolErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProtocolErrorCode {
    InvalidRequest,
    NotFound,
    Conflict,
    PermissionDenied,
    DaemonUnavailable,
    DockerUnavailable,
    FuseUnavailable,
    ProviderUnavailable,
    OperationTimedOut,
    Cancelled,
    ResourceBusy,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SubscriptionRequest {
    DaemonStatus,
    MountStatus,
    ResourceChanges {
        kinds: Vec<ResourceKind>,
    },
    ProviderStatus {
        resource: DockerResourceRef,
    },
    ContainerStats {
        container_ids: Vec<String>,
    },
    ContainerLogs {
        container_ids: Vec<String>,
        options: ContainerLogsOptions,
    },
    ImagePull {
        options: PullImageOptions,
    },
    ContainerTerminal {
        container_id: String,
        rows: u16,
        cols: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionAccepted {
    pub subscription_id: SubscriptionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResourceKind {
    Container,
    Image,
    Volume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ServerEvent {
    DaemonStatus {
        subscription_id: SubscriptionId,
        status: DaemonStatus,
    },
    MountStatus {
        subscription_id: SubscriptionId,
        status: MountStatus,
    },
    ResourceChanged {
        subscription_id: SubscriptionId,
        kind: ResourceKind,
        resource: Option<DockerResourceRef>,
        change: ResourceChange,
    },
    ProviderStatus {
        subscription_id: SubscriptionId,
        path: ResourcePath,
        descriptor: ProviderDescriptor,
    },
    ContainerStats {
        subscription_id: SubscriptionId,
        container_id: String,
        stats: ContainerStats,
    },
    ContainerLog {
        subscription_id: SubscriptionId,
        container_id: String,
        line: LogLine,
    },
    ImagePullProgress {
        subscription_id: SubscriptionId,
        progress: ImagePullProgress,
    },
    TerminalOutput {
        subscription_id: SubscriptionId,
        bytes: Vec<u8>,
    },
    TerminalState {
        subscription_id: SubscriptionId,
        state: TerminalState,
    },
    SubscriptionEnded {
        subscription_id: SubscriptionId,
        reason: SubscriptionEndReason,
    },
}

impl ServerEvent {
    #[must_use]
    pub const fn subscription_id(&self) -> SubscriptionId {
        match self {
            Self::DaemonStatus {
                subscription_id, ..
            }
            | Self::MountStatus {
                subscription_id, ..
            }
            | Self::ResourceChanged {
                subscription_id, ..
            }
            | Self::ProviderStatus {
                subscription_id, ..
            }
            | Self::ContainerStats {
                subscription_id, ..
            }
            | Self::ContainerLog {
                subscription_id, ..
            }
            | Self::ImagePullProgress {
                subscription_id, ..
            }
            | Self::TerminalOutput {
                subscription_id, ..
            }
            | Self::TerminalState {
                subscription_id, ..
            }
            | Self::SubscriptionEnded {
                subscription_id, ..
            } => *subscription_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TerminalState {
    Connecting,
    Running { shell: String },
    Exited { exit_code: Option<i64> },
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResourceChange {
    Created,
    Updated,
    Removed,
    Renamed,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SubscriptionEndReason {
    Completed,
    Unsubscribed,
    ServerShutdown,
    ResourceRemoved,
    Error(ProtocolError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame length must not be zero")]
    ZeroLength,
    #[error("frame length {length} exceeds maximum {maximum}")]
    Oversized { length: u32, maximum: u32 },
    #[error("truncated frame header: expected 4 bytes, received {actual}")]
    TruncatedHeader { actual: usize },
    #[error("truncated frame body: expected {expected} bytes, received {actual}")]
    TruncatedBody { expected: u32, actual: usize },
    #[error("unsupported protocol version {actual}; expected {expected}")]
    UnknownProtocol { actual: u32, expected: u32 },
    #[error("trailing bytes after CBOR envelope: {0}")]
    TrailingBytes(usize),
    #[error("CBOR serialization failed: {0}")]
    Encode(String),
    #[error("CBOR deserialization failed: {0}")]
    Decode(String),
}

/// Encode one envelope as `u32` big-endian length followed by CBOR.
pub fn encode_frame(envelope: &ProtocolEnvelope) -> Result<Vec<u8>, FrameError> {
    encode_frame_with_limit(envelope, MAX_FRAME_SIZE)
}

/// Encode with a negotiated limit, which can only make the global bound lower.
pub fn encode_frame_with_limit(
    envelope: &ProtocolEnvelope,
    maximum: u32,
) -> Result<Vec<u8>, FrameError> {
    let maximum = maximum.min(MAX_FRAME_SIZE);
    let mut payload = Vec::new();
    ciborium::ser::into_writer(envelope, &mut payload)
        .map_err(|error| FrameError::Encode(error.to_string()))?;
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::Oversized {
        length: u32::MAX,
        maximum,
    })?;
    validate_frame_length(length, maximum)?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode exactly one complete frame. Any bytes after it are rejected.
pub fn decode_frame(frame: &[u8]) -> Result<ProtocolEnvelope, FrameError> {
    decode_frame_with_limit(frame, MAX_FRAME_SIZE)
}

/// Decode with a negotiated limit, which can only make the global bound lower.
pub fn decode_frame_with_limit(frame: &[u8], maximum: u32) -> Result<ProtocolEnvelope, FrameError> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err(FrameError::TruncatedHeader {
            actual: frame.len(),
        });
    }
    let length = u32::from_be_bytes(frame[..FRAME_HEADER_SIZE].try_into().expect("fixed header"));
    let maximum = maximum.min(MAX_FRAME_SIZE);
    validate_frame_length(length, maximum)?;

    let expected = FRAME_HEADER_SIZE + length as usize;
    if frame.len() < expected {
        return Err(FrameError::TruncatedBody {
            expected: length,
            actual: frame.len() - FRAME_HEADER_SIZE,
        });
    }
    if frame.len() > expected {
        return Err(FrameError::TrailingBytes(frame.len() - expected));
    }
    decode_payload(&frame[FRAME_HEADER_SIZE..], length)
}

/// Decode a payload after an async transport has already read its prefix and
/// exact body. This still checks CBOR trailing bytes and protocol version.
pub fn decode_payload(
    payload: &[u8],
    declared_length: u32,
) -> Result<ProtocolEnvelope, FrameError> {
    validate_frame_length(declared_length, MAX_FRAME_SIZE)?;
    if payload.len() != declared_length as usize {
        return Err(FrameError::TruncatedBody {
            expected: declared_length,
            actual: payload.len(),
        });
    }

    let mut cursor = Cursor::new(payload);
    let envelope: ProtocolEnvelope = ciborium::de::from_reader(&mut cursor)
        .map_err(|error| FrameError::Decode(error.to_string()))?;
    let consumed = cursor.position() as usize;
    if consumed != payload.len() {
        return Err(FrameError::TrailingBytes(payload.len() - consumed));
    }
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(FrameError::UnknownProtocol {
            actual: envelope.protocol_version,
            expected: PROTOCOL_VERSION,
        });
    }
    Ok(envelope)
}

pub fn validate_frame_length(length: u32, maximum: u32) -> Result<(), FrameError> {
    if length == 0 {
        return Err(FrameError::ZeroLength);
    }
    let maximum = maximum.min(MAX_FRAME_SIZE);
    if length > maximum {
        return Err(FrameError::Oversized { length, maximum });
    }
    Ok(())
}

/// Read one synchronous frame without allocating beyond the validated bound.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<ProtocolEnvelope, ReadFrameError> {
    let mut header = [0_u8; FRAME_HEADER_SIZE];
    read_exact_counted(reader, &mut header, true)?;
    let length = u32::from_be_bytes(header);
    validate_frame_length(length, MAX_FRAME_SIZE)?;
    let mut payload = vec![0_u8; length as usize];
    read_exact_counted(reader, &mut payload, false)?;
    Ok(decode_payload(&payload, length)?)
}

fn read_exact_counted<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    header: bool,
) -> Result<(), ReadFrameError> {
    let mut read = 0;
    while read < buffer.len() {
        match reader.read(&mut buffer[read..]) {
            Ok(0) => {
                let error = if header {
                    FrameError::TruncatedHeader { actual: read }
                } else {
                    FrameError::TruncatedBody {
                        expected: buffer.len() as u32,
                        actual: read,
                    }
                };
                return Err(error.into());
            }
            Ok(count) => read += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(ReadFrameError::Io(error)),
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ReadFrameError {
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("I/O while reading frame: {0}")]
    Io(#[from] std::io::Error),
}

impl fmt::Display for FeatureFlags {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:016x}", self.0)
    }
}
