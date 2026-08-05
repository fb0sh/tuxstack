//! Streaming container root-filesystem snapshots and file transfer support.
//!
//! A Docker container export is a point-in-time view of the merged rootfs. It
//! is not a live filesystem and mounted content is not reliably represented by
//! the export endpoint. The index therefore overlays inspect-derived mount
//! destinations and hides exported descendants shadowed by those mounts.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet, hash_map::DefaultHasher};
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bollard::models::MountPoint;
use bollard::query_parameters::DownloadFromContainerOptionsBuilder;
use bytes::{Bytes, BytesMut};
use chrono::{DateTime, TimeDelta, Utc};
use futures_util::{Stream, StreamExt};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};

const TAR_BLOCK: usize = 512;
const DEFAULT_SNAPSHOT_TTL: Duration = Duration::from_secs(10);
const DEFAULT_PREVIEW_BYTES: usize = 1024 * 1024;

type ArchiveByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, ContainerFilesystemError>> + Send>>;

/// Hard limits applied while parsing daemon-provided tar streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFilesystemLimits {
    /// Maximum number of indexed non-root entries, including synthetic mount
    /// parents. Metadata-only PAX/GNU extension records do not count.
    pub max_entries: usize,
    /// Maximum decoded raw or logical path length in bytes.
    pub max_path_bytes: usize,
    /// Maximum payload accepted for a single PAX or GNU long-name record.
    pub max_extension_bytes: usize,
    /// Default number of file bytes retained by preview.
    pub max_preview_bytes: usize,
}

impl Default for ContainerFilesystemLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_path_bytes: 4096,
            max_extension_bytes: 1024 * 1024,
            max_preview_bytes: DEFAULT_PREVIEW_BYTES,
        }
    }
}

/// Stable failures exposed by the container-files backend.
#[derive(Debug, thiserror::Error)]
pub enum ContainerFilesystemError {
    #[error("container filesystem operation was cancelled")]
    Cancelled,
    #[error("container filesystem operation timed out")]
    Timeout,
    #[error("Docker operation failed: {0}")]
    Docker(#[source] DockerError),
    #[error("invalid container path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("tar archive is malformed: {0}")]
    MalformedArchive(String),
    #[error("tar archive stream was truncated")]
    TruncatedArchive,
    #[error("tar archive contains more than {limit} index entries")]
    EntryLimitExceeded { limit: usize },
    #[error("tar path exceeds the {limit}-byte limit")]
    PathTooLong { limit: usize },
    #[error("tar extension record exceeds the {limit}-byte limit")]
    ExtensionTooLarge { limit: usize },
    #[error("tar archive contains duplicate path: {0}")]
    DuplicatePath(String),
    #[error("archive entry does not match requested path {requested:?}: {actual:?}")]
    UnexpectedArchiveEntry { requested: String, actual: String },
    #[error("download archive is ambiguous: {0}")]
    AmbiguousArchive(String),
    #[error("requested archive entry is not a regular file: {0}")]
    NotRegularFile(String),
    #[error("invalid directory cursor")]
    InvalidCursor,
    #[error("filesystem I/O failed for {path}: {message}")]
    Io { path: PathBuf, message: String },
}

/// Kind of an inspect-derived mount overlay.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContainerMountOverlayKind {
    Volume,
    Bind,
    Tmpfs,
    Other(String),
}

/// A mounted destination which supersedes exported rootfs content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerMountOverlay {
    pub kind: ContainerMountOverlayKind,
    /// Absolute, normalized path inside the container.
    pub destination: String,
    /// Volume name, bind source, or daemon-provided source when applicable.
    pub source: Option<String>,
    pub read_only: bool,
}

impl ContainerMountOverlay {
    pub fn new(
        kind: ContainerMountOverlayKind,
        destination: impl Into<String>,
        source: Option<String>,
        read_only: bool,
    ) -> Result<Self, ContainerFilesystemError> {
        let destination = normalize_absolute_path(&destination.into())?;
        Ok(Self {
            kind,
            destination,
            source,
            read_only,
        })
    }
}

/// Tar entry kind retained by the in-memory index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContainerFilesystemEntryType {
    File,
    Directory,
    Symlink,
    Hardlink,
    Other,
}

/// Provenance of an indexed path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContainerFilesystemOrigin {
    /// Entry came from the merged rootfs export.
    RootFilesystem,
    /// Entry is a synthetic parent needed to make the index navigable.
    SyntheticParent,
    /// Entry is on the synthetic navigation route to a mount destination.
    MountRoute { mount_index: usize },
    /// This exact path is an inspect-derived mount destination.
    MountOverlay { mount_index: usize },
    /// Exported content is hidden by a mount and is not runtime mount data.
    ShadowedByMount { mount_index: usize },
}

impl ContainerFilesystemOrigin {
    pub fn is_shadowed(&self) -> bool {
        matches!(self, Self::ShadowedByMount { .. })
    }
}

/// A single entry in a rootfs snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFilesystemEntry {
    /// Path as represented by the tar header after PAX/GNU name expansion.
    pub raw_path: String,
    /// Normalized absolute path used by directory queries.
    pub logical_path: String,
    pub name: String,
    pub display_name: String,
    pub entry_type: ContainerFilesystemEntryType,
    pub size: u64,
    pub mode: u32,
    pub uid: u64,
    pub gid: u64,
    pub mtime: Option<DateTime<Utc>>,
    pub link_target: Option<String>,
    pub origin: ContainerFilesystemOrigin,
}

/// Sort key for directory queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerDirectorySort {
    Name,
    Size,
    Modified,
    Type,
}

/// Sort direction for directory queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerDirectorySortOrder {
    Ascending,
    Descending,
}

/// Directory listing options. Shadowed rootfs descendants are hidden by
/// default because they must never be presented as live mounted content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerDirectoryQuery {
    pub directory: String,
    pub include_hidden: bool,
    pub include_shadowed: bool,
    pub sort: ContainerDirectorySort,
    pub order: ContainerDirectorySortOrder,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for ContainerDirectoryQuery {
    fn default() -> Self {
        Self {
            directory: "/".to_string(),
            include_hidden: false,
            include_shadowed: false,
            sort: ContainerDirectorySort::Name,
            order: ContainerDirectorySortOrder::Ascending,
            limit: 200,
            cursor: None,
        }
    }
}

/// One page from the immutable snapshot index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerDirectoryPage {
    pub directory: String,
    pub entries: Vec<ContainerFilesystemEntry>,
    pub next_cursor: Option<String>,
    pub total_visible: usize,
}

/// Point-in-time merged-rootfs index. It is deliberately mutable in meaning:
/// callers should use `is_fresh` and explicitly refresh rather than treating
/// this snapshot as a live view. The service does not implement GUI caching.
#[derive(Debug, Clone)]
pub struct ContainerFilesystemSnapshot {
    pub container_id: String,
    pub generated_at: DateTime<Utc>,
    pub ttl: Duration,
    pub entries: Vec<ContainerFilesystemEntry>,
    pub mount_overlays: Vec<ContainerMountOverlay>,
    children: BTreeMap<String, Vec<usize>>,
}

impl ContainerFilesystemSnapshot {
    pub fn is_fresh(&self) -> bool {
        self.is_fresh_at(Utc::now())
    }

    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        let age = now.signed_duration_since(self.generated_at);
        age <= TimeDelta::zero() || age.to_std().is_ok_and(|age| age <= self.ttl)
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.generated_at + TimeDelta::from_std(self.ttl).unwrap_or(TimeDelta::MAX)
    }

    pub fn entry(
        &self,
        path: &str,
    ) -> Result<Option<&ContainerFilesystemEntry>, ContainerFilesystemError> {
        let path = normalize_absolute_path(path)?;
        Ok(self.entries.iter().find(|entry| entry.logical_path == path))
    }

    pub fn list_directory(
        &self,
        query: &ContainerDirectoryQuery,
    ) -> Result<ContainerDirectoryPage, ContainerFilesystemError> {
        let directory = normalize_absolute_path(&query.directory)?;
        let mut entries: Vec<ContainerFilesystemEntry> = self
            .children
            .get(&directory)
            .into_iter()
            .flatten()
            .filter_map(|index| self.entries.get(*index))
            .filter(|entry| query.include_hidden || !entry.name.starts_with('.'))
            .filter(|entry| query.include_shadowed || !entry.origin.is_shadowed())
            .cloned()
            .collect();

        entries.sort_by(|left, right| compare_entries(left, right, query.sort));
        if query.order == ContainerDirectorySortOrder::Descending {
            entries.reverse();
        }

        let identity = cursor_identity(&directory, query);
        let offset = match query.cursor.as_deref() {
            None => 0,
            Some(cursor) => decode_cursor(cursor, self.generated_at, identity)?,
        };
        if offset > entries.len() {
            return Err(ContainerFilesystemError::InvalidCursor);
        }

        let total_visible = entries.len();
        let limit = query.limit.max(1);
        let end = offset.saturating_add(limit).min(entries.len());
        let page_entries = entries[offset..end].to_vec();
        let next_cursor =
            (end < entries.len()).then(|| encode_cursor(end, self.generated_at, identity));

        Ok(ContainerDirectoryPage {
            directory,
            entries: page_entries,
            next_cursor,
            total_visible,
        })
    }

    fn rebuild_children(&mut self) {
        self.children.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.logical_path == "/" {
                continue;
            }
            self.children
                .entry(parent_path(&entry.logical_path))
                .or_default()
                .push(index);
        }
    }
}

/// Result of streaming a downloaded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFileTransfer {
    pub path: String,
    pub bytes_written: u64,
    pub file_size: u64,
}

/// Bounded preview payload. `truncated` means the file is larger than `bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFilePreview {
    pub path: String,
    pub bytes: Vec<u8>,
    pub file_size: u64,
    pub truncated: bool,
}

/// Docker-backed rootfs snapshot and archive-copy service.
#[derive(Clone)]
pub struct ContainerFilesystemService {
    client: Arc<DockerClient>,
    limits: ContainerFilesystemLimits,
    operation_timeout: Duration,
    snapshot_ttl: Duration,
}

impl ContainerFilesystemService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        let operation_timeout = client.config().request_timeout;
        Self {
            client,
            limits: ContainerFilesystemLimits::default(),
            operation_timeout,
            snapshot_ttl: DEFAULT_SNAPSHOT_TTL,
        }
    }

    pub fn with_options(
        client: Arc<DockerClient>,
        limits: ContainerFilesystemLimits,
        operation_timeout: Duration,
        snapshot_ttl: Duration,
    ) -> Self {
        Self {
            client,
            limits,
            operation_timeout,
            snapshot_ttl,
        }
    }

    /// Inspect mounts, then stream and index the merged rootfs export.
    pub async fn snapshot(
        &self,
        container_id: &str,
        cancellation: CancellationToken,
    ) -> Result<ContainerFilesystemSnapshot, ContainerFilesystemError> {
        validate_container_id(container_id)?;
        let docker = self
            .client
            .inner()
            .clone()
            .with_timeout(self.operation_timeout);
        let container_id = container_id.to_string();
        let limits = self.limits.clone();
        let ttl = self.snapshot_ttl;

        run_cancellable(self.operation_timeout, &cancellation, async move {
            let inspect = docker
                .inspect_container(&container_id, None)
                .await
                .map_err(|error| docker_error(error, "container"))?;
            let overlays = map_mount_overlays(inspect.mounts.unwrap_or_default())?;
            let stream = docker
                .export_container(&container_id)
                .map(|item| item.map_err(|error| docker_error(error, "container export")));
            build_snapshot_from_stream(&container_id, overlays, ttl, limits, Box::pin(stream)).await
        })
        .await
    }

    /// Build a snapshot with caller-provided inspect-derived overlays. Useful
    /// when the caller already has a fresh inspect result.
    pub async fn snapshot_with_mounts(
        &self,
        container_id: &str,
        overlays: Vec<ContainerMountOverlay>,
        cancellation: CancellationToken,
    ) -> Result<ContainerFilesystemSnapshot, ContainerFilesystemError> {
        validate_container_id(container_id)?;
        let docker = self
            .client
            .inner()
            .clone()
            .with_timeout(self.operation_timeout);
        let container_id = container_id.to_string();
        let limits = self.limits.clone();
        let ttl = self.snapshot_ttl;
        validate_overlays(&overlays)?;

        run_cancellable(self.operation_timeout, &cancellation, async move {
            let stream = docker
                .export_container(&container_id)
                .map(|item| item.map_err(|error| docker_error(error, "container export")));
            build_snapshot_from_stream(&container_id, overlays, ttl, limits, Box::pin(stream)).await
        })
        .await
    }

    /// Stream one regular file from Docker's archive endpoint into a caller-
    /// supplied writer. No payload is collected in memory.
    pub async fn download_file_to_writer<W>(
        &self,
        container_id: &str,
        container_path: &str,
        writer: &mut W,
        cancellation: CancellationToken,
    ) -> Result<ContainerFileTransfer, ContainerFilesystemError>
    where
        W: AsyncWrite + Unpin,
    {
        validate_container_id(container_id)?;
        let requested = normalize_absolute_path(container_path)?;
        if requested == "/" {
            return Err(ContainerFilesystemError::NotRegularFile(requested));
        }
        let options = DownloadFromContainerOptionsBuilder::default()
            .path(&requested)
            .build();
        let docker = self
            .client
            .inner()
            .clone()
            .with_timeout(self.operation_timeout);
        let stream = docker
            .download_from_container(container_id, Some(options))
            .map(|item| item.map_err(|error| docker_error(error, "container archive")));
        let limits = self.limits.clone();

        run_cancellable(self.operation_timeout, &cancellation, async {
            copy_single_file_archive(Box::pin(stream), &requested, writer, None, &limits).await
        })
        .await
    }

    /// Download a bounded prefix of one regular file. The remainder is
    /// streamed past without retention so archive truncation/ambiguity is
    /// still detected.
    pub async fn preview_file(
        &self,
        container_id: &str,
        container_path: &str,
        max_bytes: Option<usize>,
        cancellation: CancellationToken,
    ) -> Result<ContainerFilePreview, ContainerFilesystemError> {
        let requested = normalize_absolute_path(container_path)?;
        let max_bytes = max_bytes
            .unwrap_or(self.limits.max_preview_bytes)
            .min(self.limits.max_preview_bytes);
        let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
        let transfer = self
            .download_file_limited(
                container_id,
                &requested,
                &mut bytes,
                max_bytes as u64,
                cancellation,
            )
            .await?;
        Ok(ContainerFilePreview {
            path: requested,
            truncated: transfer.file_size > transfer.bytes_written,
            file_size: transfer.file_size,
            bytes,
        })
    }

    async fn download_file_limited<W>(
        &self,
        container_id: &str,
        requested: &str,
        writer: &mut W,
        max_bytes: u64,
        cancellation: CancellationToken,
    ) -> Result<ContainerFileTransfer, ContainerFilesystemError>
    where
        W: AsyncWrite + Unpin,
    {
        validate_container_id(container_id)?;
        let options = DownloadFromContainerOptionsBuilder::default()
            .path(requested)
            .build();
        let docker = self
            .client
            .inner()
            .clone()
            .with_timeout(self.operation_timeout);
        let stream = docker
            .download_from_container(container_id, Some(options))
            .map(|item| item.map_err(|error| docker_error(error, "container archive")));
        let limits = self.limits.clone();
        run_cancellable(self.operation_timeout, &cancellation, async {
            copy_single_file_archive(
                Box::pin(stream),
                requested,
                writer,
                Some(max_bytes),
                &limits,
            )
            .await
        })
        .await
    }

    /// Save through a unique sibling `.part` file and atomically rename only
    /// after a complete, validated archive has been received.
    pub async fn save_file(
        &self,
        container_id: &str,
        container_path: &str,
        destination: impl AsRef<Path>,
        cancellation: CancellationToken,
    ) -> Result<ContainerFileTransfer, ContainerFilesystemError> {
        let destination = destination.as_ref().to_path_buf();
        let temporary = part_path(&destination)?;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
            .map_err(|error| io_error(&temporary, error))?;

        let transfer = match self
            .download_file_to_writer(container_id, container_path, &mut file, cancellation)
            .await
        {
            Ok(transfer) => transfer,
            Err(error) => {
                drop(file);
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error);
            }
        };

        if let Err(error) = file.flush().await {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(io_error(&temporary, error));
        }
        drop(file);
        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(io_error(&destination, error));
        }
        Ok(transfer)
    }
}

async fn run_cancellable<T>(
    timeout: Duration,
    cancellation: &CancellationToken,
    future: impl std::future::Future<Output = Result<T, ContainerFilesystemError>>,
) -> Result<T, ContainerFilesystemError> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ContainerFilesystemError::Cancelled),
        result = tokio::time::timeout(timeout, future) => {
            result.map_err(|_| ContainerFilesystemError::Timeout)?
        }
    }
}

fn validate_container_id(container_id: &str) -> Result<(), ContainerFilesystemError> {
    if container_id.trim().is_empty() || container_id.as_bytes().contains(&0) {
        return Err(ContainerFilesystemError::InvalidPath {
            path: container_id.to_string(),
            reason: "container id is empty or contains NUL",
        });
    }
    Ok(())
}

fn docker_error(error: bollard::errors::Error, resource: &str) -> ContainerFilesystemError {
    ContainerFilesystemError::Docker(classify_api_error(&error, resource))
}

fn io_error(path: &Path, error: std::io::Error) -> ContainerFilesystemError {
    ContainerFilesystemError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn part_path(destination: &Path) -> Result<PathBuf, ContainerFilesystemError> {
    let name = destination
        .file_name()
        .ok_or_else(|| ContainerFilesystemError::InvalidPath {
            path: destination.display().to_string(),
            reason: "destination has no file name",
        })?;
    let mut part_name = OsString::from(".tuxstack-");
    part_name.push(name);
    part_name.push(format!("-{}.part", uuid::Uuid::new_v4()));
    Ok(destination.with_file_name(part_name))
}

fn map_mount_overlays(
    mounts: Vec<MountPoint>,
) -> Result<Vec<ContainerMountOverlay>, ContainerFilesystemError> {
    mounts
        .into_iter()
        .filter_map(|mount| {
            let destination = mount.destination?;
            let raw_kind = mount.typ.unwrap_or_default();
            let kind = match raw_kind.as_str() {
                "volume" => ContainerMountOverlayKind::Volume,
                "bind" => ContainerMountOverlayKind::Bind,
                "tmpfs" => ContainerMountOverlayKind::Tmpfs,
                other => ContainerMountOverlayKind::Other(other.to_string()),
            };
            let source = if matches!(kind, ContainerMountOverlayKind::Volume) {
                mount.name.or(mount.source)
            } else {
                mount.source
            };
            Some(ContainerMountOverlay::new(
                kind,
                destination,
                source,
                !mount.rw.unwrap_or(false),
            ))
        })
        .collect()
}

fn validate_overlays(overlays: &[ContainerMountOverlay]) -> Result<(), ContainerFilesystemError> {
    let mut destinations = HashSet::new();
    for overlay in overlays {
        let normalized = normalize_absolute_path(&overlay.destination)?;
        if normalized != overlay.destination {
            return Err(ContainerFilesystemError::InvalidPath {
                path: overlay.destination.clone(),
                reason: "mount destination is not normalized",
            });
        }
        if normalized == "/" {
            return Err(ContainerFilesystemError::InvalidPath {
                path: normalized,
                reason: "root mount overlays are unsupported",
            });
        }
        if !destinations.insert(overlay.destination.as_str()) {
            return Err(ContainerFilesystemError::InvalidPath {
                path: overlay.destination.clone(),
                reason: "duplicate mount destination",
            });
        }
    }
    Ok(())
}

fn compare_entries(
    left: &ContainerFilesystemEntry,
    right: &ContainerFilesystemEntry,
    sort: ContainerDirectorySort,
) -> Ordering {
    let primary = match sort {
        ContainerDirectorySort::Name => left
            .display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase()),
        ContainerDirectorySort::Size => left.size.cmp(&right.size),
        ContainerDirectorySort::Modified => left.mtime.cmp(&right.mtime),
        ContainerDirectorySort::Type => left.entry_type.cmp(&right.entry_type),
    };
    primary
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.logical_path.cmp(&right.logical_path))
}

fn cursor_identity(directory: &str, query: &ContainerDirectoryQuery) -> u64 {
    let mut hasher = DefaultHasher::new();
    directory.hash(&mut hasher);
    query.include_hidden.hash(&mut hasher);
    query.include_shadowed.hash(&mut hasher);
    query.sort.hash(&mut hasher);
    query.order.hash(&mut hasher);
    hasher.finish()
}

fn encode_cursor(offset: usize, generated_at: DateTime<Utc>, identity: u64) -> String {
    format!(
        "v1:{}:{identity:016x}:{offset}",
        generated_at.timestamp_nanos_opt().unwrap_or_default()
    )
}

fn decode_cursor(
    cursor: &str,
    generated_at: DateTime<Utc>,
    identity: u64,
) -> Result<usize, ContainerFilesystemError> {
    let mut parts = cursor.split(':');
    let version = parts.next();
    let timestamp = parts.next().and_then(|value| value.parse::<i64>().ok());
    let encoded_identity = parts
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    let offset = parts.next().and_then(|value| value.parse::<usize>().ok());
    if version != Some("v1")
        || timestamp != generated_at.timestamp_nanos_opt()
        || encoded_identity != Some(identity)
        || parts.next().is_some()
    {
        return Err(ContainerFilesystemError::InvalidCursor);
    }
    offset.ok_or(ContainerFilesystemError::InvalidCursor)
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
        .to_string()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_descendant(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len()
        && path.starts_with(ancestor)
        && (ancestor == "/" || path.as_bytes().get(ancestor.len()) == Some(&b'/'))
}

fn normalize_absolute_path(path: &str) -> Result<String, ContainerFilesystemError> {
    if path.as_bytes().contains(&0) {
        return Err(ContainerFilesystemError::InvalidPath {
            path: path.to_string(),
            reason: "NUL byte is forbidden",
        });
    }
    if !path.starts_with('/') {
        return Err(ContainerFilesystemError::InvalidPath {
            path: path.to_string(),
            reason: "path must be absolute",
        });
    }
    normalize_components(path, true)
}

fn normalize_tar_path(
    raw: &str,
    max_path_bytes: usize,
) -> Result<String, ContainerFilesystemError> {
    if raw.len() > max_path_bytes {
        return Err(ContainerFilesystemError::PathTooLong {
            limit: max_path_bytes,
        });
    }
    if raw.as_bytes().contains(&0) {
        return Err(ContainerFilesystemError::InvalidPath {
            path: raw.to_string(),
            reason: "NUL byte is forbidden",
        });
    }
    if raw.starts_with('/') {
        return Err(ContainerFilesystemError::InvalidPath {
            path: raw.to_string(),
            reason: "absolute tar paths are forbidden",
        });
    }
    let normalized = normalize_components(raw, false)?;
    if normalized.len() > max_path_bytes {
        return Err(ContainerFilesystemError::PathTooLong {
            limit: max_path_bytes,
        });
    }
    Ok(normalized)
}

fn normalize_components(
    path: &str,
    already_absolute: bool,
) -> Result<String, ContainerFilesystemError> {
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return Err(ContainerFilesystemError::InvalidPath {
                    path: path.to_string(),
                    reason: "parent traversal is forbidden",
                });
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        return Ok("/".to_string());
    }
    let joined = components.join("/");
    if already_absolute || !joined.is_empty() {
        Ok(format!("/{joined}"))
    } else {
        Ok("/".to_string())
    }
}

struct ArchiveReader {
    stream: ArchiveByteStream,
    buffer: BytesMut,
    stream_ended: bool,
}

impl ArchiveReader {
    fn new(stream: ArchiveByteStream) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
            stream_ended: false,
        }
    }

    async fn fill(&mut self) -> Result<(), ContainerFilesystemError> {
        while self.buffer.is_empty() && !self.stream_ended {
            match self.stream.next().await {
                Some(Ok(bytes)) if bytes.is_empty() => continue,
                Some(Ok(bytes)) => self.buffer.extend_from_slice(&bytes),
                Some(Err(error)) => return Err(error),
                None => self.stream_ended = true,
            }
        }
        Ok(())
    }

    async fn read_exact(&mut self, length: usize) -> Result<Vec<u8>, ContainerFilesystemError> {
        let mut output = Vec::with_capacity(length);
        while output.len() < length {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Err(ContainerFilesystemError::TruncatedArchive);
            }
            let take = (length - output.len()).min(self.buffer.len());
            output.extend_from_slice(&self.buffer.split_to(take));
        }
        Ok(output)
    }

    async fn skip_exact(&mut self, mut length: u64) -> Result<(), ContainerFilesystemError> {
        while length > 0 {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Err(ContainerFilesystemError::TruncatedArchive);
            }
            let take = length.min(self.buffer.len() as u64) as usize;
            let _ = self.buffer.split_to(take);
            length -= take as u64;
        }
        Ok(())
    }

    async fn ensure_zero_tail(&mut self) -> Result<(), ContainerFilesystemError> {
        loop {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Ok(());
            }
            if self.buffer.iter().any(|byte| *byte != 0) {
                return Err(ContainerFilesystemError::MalformedArchive(
                    "non-zero data follows tar end marker".to_string(),
                ));
            }
            self.buffer.clear();
        }
    }

    async fn copy_exact<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        mut length: u64,
        limit: Option<u64>,
        output_path: &str,
    ) -> Result<u64, ContainerFilesystemError> {
        let mut written = 0u64;
        while length > 0 {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Err(ContainerFilesystemError::TruncatedArchive);
            }
            let take = length.min(self.buffer.len() as u64) as usize;
            let chunk = self.buffer.split_to(take);
            let allowed = limit
                .map(|limit| limit.saturating_sub(written).min(take as u64) as usize)
                .unwrap_or(take);
            if allowed > 0 {
                writer
                    .write_all(&chunk[..allowed])
                    .await
                    .map_err(|error| io_error(Path::new(output_path), error))?;
                written += allowed as u64;
            }
            length -= take as u64;
        }
        Ok(written)
    }
}

#[derive(Debug)]
struct TarHeader {
    raw_path: String,
    entry_type: ContainerFilesystemEntryType,
    type_flag: u8,
    size: u64,
    mode: u64,
    uid: u64,
    gid: u64,
    mtime: i64,
    link_target: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct PaxValues {
    path: Option<String>,
    link_path: Option<String>,
    size: Option<u64>,
    mtime: Option<i64>,
    uid: Option<u64>,
    gid: Option<u64>,
}

impl PaxValues {
    fn overlay(&mut self, other: PaxValues) {
        if other.path.is_some() {
            self.path = other.path;
        }
        if other.link_path.is_some() {
            self.link_path = other.link_path;
        }
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.mtime.is_some() {
            self.mtime = other.mtime;
        }
        if other.uid.is_some() {
            self.uid = other.uid;
        }
        if other.gid.is_some() {
            self.gid = other.gid;
        }
    }
}

fn parse_header(block: &[u8]) -> Result<TarHeader, ContainerFilesystemError> {
    if block.len() != TAR_BLOCK {
        return Err(ContainerFilesystemError::TruncatedArchive);
    }
    verify_checksum(block)?;
    let name = parse_text_field(&block[0..100], "name")?;
    let prefix = parse_text_field(&block[345..500], "prefix")?;
    let raw_path = if prefix.is_empty() {
        name
    } else if name.is_empty() {
        prefix
    } else {
        format!("{prefix}/{name}")
    };
    if raw_path.is_empty() {
        return Err(ContainerFilesystemError::MalformedArchive(
            "entry path is empty".to_string(),
        ));
    }
    let type_flag = block[156];
    let entry_type = match type_flag {
        0 | b'0' | b'7' => ContainerFilesystemEntryType::File,
        b'5' => ContainerFilesystemEntryType::Directory,
        b'2' => ContainerFilesystemEntryType::Symlink,
        b'1' => ContainerFilesystemEntryType::Hardlink,
        _ => ContainerFilesystemEntryType::Other,
    };
    let link = parse_text_field(&block[157..257], "link name")?;
    Ok(TarHeader {
        raw_path,
        entry_type,
        type_flag,
        size: parse_tar_number(&block[124..136], "size")?,
        mode: parse_tar_number(&block[100..108], "mode")?,
        uid: parse_tar_number(&block[108..116], "uid")?,
        gid: parse_tar_number(&block[116..124], "gid")?,
        mtime: parse_tar_number(&block[136..148], "mtime")?
            .try_into()
            .map_err(|_| {
                ContainerFilesystemError::MalformedArchive("mtime is out of range".to_string())
            })?,
        link_target: (!link.is_empty()).then_some(link),
    })
}

fn parse_text_field(field: &[u8], label: &str) -> Result<String, ContainerFilesystemError> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if end < field.len() && field[end + 1..].iter().any(|byte| *byte != 0) {
        return Err(ContainerFilesystemError::MalformedArchive(format!(
            "{label} contains data after NUL"
        )));
    }
    std::str::from_utf8(&field[..end])
        .map(str::to_string)
        .map_err(|_| ContainerFilesystemError::MalformedArchive(format!("{label} is not UTF-8")))
}

fn parse_tar_number(field: &[u8], label: &str) -> Result<u64, ContainerFilesystemError> {
    if field.is_empty() {
        return Err(ContainerFilesystemError::MalformedArchive(format!(
            "{label} field is empty"
        )));
    }
    if field[0] & 0x80 != 0 {
        if field[0] & 0x40 != 0 {
            return Err(ContainerFilesystemError::MalformedArchive(format!(
                "negative base-256 {label} is unsupported"
            )));
        }
        let mut value = (field[0] & 0x3f) as u64;
        for byte in &field[1..] {
            value = value
                .checked_mul(256)
                .and_then(|value| value.checked_add(*byte as u64))
                .ok_or_else(|| {
                    ContainerFilesystemError::MalformedArchive(format!(
                        "base-256 {label} overflows u64"
                    ))
                })?;
        }
        return Ok(value);
    }

    let text = std::str::from_utf8(field)
        .map_err(|_| ContainerFilesystemError::MalformedArchive(format!("{label} is not ASCII")))?;
    let text = text.trim_matches(|character| character == '\0' || character == ' ');
    if text.is_empty() {
        return Ok(0);
    }
    if !text.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err(ContainerFilesystemError::MalformedArchive(format!(
            "{label} is not a valid octal number"
        )));
    }
    u64::from_str_radix(text, 8)
        .map_err(|_| ContainerFilesystemError::MalformedArchive(format!("{label} overflows u64")))
}

fn verify_checksum(block: &[u8]) -> Result<(), ContainerFilesystemError> {
    let expected = parse_tar_number(&block[148..156], "checksum")?;
    let actual: u64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                b' ' as u64
            } else {
                *byte as u64
            }
        })
        .sum();
    if expected != actual {
        return Err(ContainerFilesystemError::MalformedArchive(
            "header checksum mismatch".to_string(),
        ));
    }
    Ok(())
}

fn padding(size: u64) -> u64 {
    (TAR_BLOCK as u64 - size % TAR_BLOCK as u64) % TAR_BLOCK as u64
}

fn parse_pax(data: &[u8]) -> Result<PaxValues, ContainerFilesystemError> {
    let mut values = PaxValues::default();
    let mut offset = 0usize;
    while offset < data.len() {
        let space = data[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|position| offset + position)
            .ok_or_else(|| {
                ContainerFilesystemError::MalformedArchive(
                    "PAX record has no length separator".to_string(),
                )
            })?;
        let length_text = std::str::from_utf8(&data[offset..space]).map_err(|_| {
            ContainerFilesystemError::MalformedArchive("PAX length is not ASCII".to_string())
        })?;
        if length_text.is_empty() || !length_text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ContainerFilesystemError::MalformedArchive(
                "PAX record length is invalid".to_string(),
            ));
        }
        let length: usize = length_text.parse().map_err(|_| {
            ContainerFilesystemError::MalformedArchive("PAX record length overflows".to_string())
        })?;
        let end = offset.checked_add(length).ok_or_else(|| {
            ContainerFilesystemError::MalformedArchive("PAX record length overflows".to_string())
        })?;
        if length == 0 || end > data.len() || data[end - 1] != b'\n' || space + 1 >= end {
            return Err(ContainerFilesystemError::MalformedArchive(
                "PAX record length does not match payload".to_string(),
            ));
        }
        let record = &data[space + 1..end - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| {
                ContainerFilesystemError::MalformedArchive("PAX record has no '='".to_string())
            })?;
        let key = std::str::from_utf8(&record[..equals]).map_err(|_| {
            ContainerFilesystemError::MalformedArchive("PAX key is not UTF-8".to_string())
        })?;
        let value = std::str::from_utf8(&record[equals + 1..]).map_err(|_| {
            ContainerFilesystemError::MalformedArchive("PAX value is not UTF-8".to_string())
        })?;
        if value.as_bytes().contains(&0) {
            return Err(ContainerFilesystemError::MalformedArchive(
                "PAX value contains NUL".to_string(),
            ));
        }
        match key {
            "path" => values.path = Some(value.to_string()),
            "linkpath" => values.link_path = Some(value.to_string()),
            "size" => values.size = Some(parse_pax_u64(value, "size")?),
            "mtime" => values.mtime = Some(parse_pax_mtime(value)?),
            "uid" => values.uid = Some(parse_pax_u64(value, "uid")?),
            "gid" => values.gid = Some(parse_pax_u64(value, "gid")?),
            _ => {}
        }
        offset = end;
    }
    Ok(values)
}

fn parse_pax_u64(value: &str, label: &str) -> Result<u64, ContainerFilesystemError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ContainerFilesystemError::MalformedArchive(format!(
            "PAX {label} is invalid"
        )));
    }
    value.parse().map_err(|_| {
        ContainerFilesystemError::MalformedArchive(format!("PAX {label} overflows u64"))
    })
}

fn parse_pax_mtime(value: &str) -> Result<i64, ContainerFilesystemError> {
    let integer = value.split('.').next().unwrap_or(value);
    integer
        .parse()
        .map_err(|_| ContainerFilesystemError::MalformedArchive("PAX mtime is invalid".to_string()))
}

async fn read_extension(
    reader: &mut ArchiveReader,
    size: u64,
    limits: &ContainerFilesystemLimits,
) -> Result<Vec<u8>, ContainerFilesystemError> {
    if size > limits.max_extension_bytes as u64 {
        return Err(ContainerFilesystemError::ExtensionTooLarge {
            limit: limits.max_extension_bytes,
        });
    }
    let data = reader.read_exact(size as usize).await?;
    reader.skip_exact(padding(size)).await?;
    Ok(data)
}

async fn build_snapshot_from_stream(
    container_id: &str,
    overlays: Vec<ContainerMountOverlay>,
    ttl: Duration,
    limits: ContainerFilesystemLimits,
    stream: ArchiveByteStream,
) -> Result<ContainerFilesystemSnapshot, ContainerFilesystemError> {
    validate_overlays(&overlays)?;
    let mut reader = ArchiveReader::new(stream);
    let mut entries = vec![root_entry()];
    let mut paths = HashSet::new();
    paths.insert("/".to_string());
    let mut global_pax = PaxValues::default();
    let mut pending_pax = PaxValues::default();
    let mut pending_long_name: Option<String> = None;
    let mut pending_long_link: Option<String> = None;
    let mut zero_blocks = 0;

    loop {
        let block = reader.read_exact(TAR_BLOCK).await?;
        if block.iter().all(|byte| *byte == 0) {
            zero_blocks += 1;
            if zero_blocks == 2 {
                reader.ensure_zero_tail().await?;
                break;
            }
            continue;
        }
        if zero_blocks != 0 {
            return Err(ContainerFilesystemError::MalformedArchive(
                "non-zero header follows tar end marker".to_string(),
            ));
        }
        let header = parse_header(&block)?;
        match header.type_flag {
            b'x' | b'g' => {
                let data = read_extension(&mut reader, header.size, &limits).await?;
                let pax = parse_pax(&data)?;
                if header.type_flag == b'g' {
                    global_pax.overlay(pax);
                } else {
                    pending_pax.overlay(pax);
                }
                continue;
            }
            b'L' | b'K' => {
                let data = read_extension(&mut reader, header.size, &limits).await?;
                let value = parse_long_value(&data)?;
                if header.type_flag == b'L' {
                    pending_long_name = Some(value);
                } else {
                    pending_long_link = Some(value);
                }
                continue;
            }
            _ => {}
        }

        let mut pax = global_pax.clone();
        pax.overlay(std::mem::take(&mut pending_pax));
        let raw_path = pax
            .path
            .take()
            .or_else(|| pending_long_name.take())
            .unwrap_or(header.raw_path);
        let logical_path = normalize_tar_path(&raw_path, limits.max_path_bytes)?;
        let link_target = pax
            .link_path
            .take()
            .or_else(|| pending_long_link.take())
            .or(header.link_target);
        validate_link_target(link_target.as_deref(), limits.max_path_bytes)?;
        let size = pax.size.unwrap_or(header.size);
        reader.skip_exact(size).await?;
        reader.skip_exact(padding(size)).await?;

        if logical_path == "/" {
            continue;
        }
        if !paths.insert(logical_path.clone()) {
            return Err(ContainerFilesystemError::DuplicatePath(logical_path));
        }
        ensure_entry_capacity(entries.len(), limits.max_entries)?;
        let name = basename(&logical_path).to_string();
        entries.push(ContainerFilesystemEntry {
            raw_path,
            logical_path,
            display_name: name.clone(),
            name,
            entry_type: header.entry_type,
            size,
            mode: header.mode.try_into().map_err(|_| {
                ContainerFilesystemError::MalformedArchive("mode is out of range".to_string())
            })?,
            uid: pax.uid.unwrap_or(header.uid),
            gid: pax.gid.unwrap_or(header.gid),
            mtime: DateTime::from_timestamp(pax.mtime.unwrap_or(header.mtime), 0),
            link_target,
            origin: ContainerFilesystemOrigin::RootFilesystem,
        });
    }

    let mut snapshot = ContainerFilesystemSnapshot {
        container_id: container_id.to_string(),
        generated_at: Utc::now(),
        ttl,
        entries,
        mount_overlays: overlays,
        children: BTreeMap::new(),
    };
    apply_mount_overlays(&mut snapshot, &limits)?;
    snapshot.rebuild_children();
    Ok(snapshot)
}

fn parse_long_value(data: &[u8]) -> Result<String, ContainerFilesystemError> {
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    if data[end..].iter().any(|byte| !matches!(*byte, 0 | b'\n')) {
        return Err(ContainerFilesystemError::MalformedArchive(
            "GNU long-name record contains data after NUL".to_string(),
        ));
    }
    let value = std::str::from_utf8(&data[..end]).map_err(|_| {
        ContainerFilesystemError::MalformedArchive("GNU long-name is not UTF-8".to_string())
    })?;
    Ok(value.trim_end_matches('\n').to_string())
}

fn validate_link_target(
    target: Option<&str>,
    max_path_bytes: usize,
) -> Result<(), ContainerFilesystemError> {
    if let Some(target) = target {
        if target.len() > max_path_bytes {
            return Err(ContainerFilesystemError::PathTooLong {
                limit: max_path_bytes,
            });
        }
        if target.as_bytes().contains(&0) {
            return Err(ContainerFilesystemError::MalformedArchive(
                "link target contains NUL".to_string(),
            ));
        }
    }
    Ok(())
}

fn ensure_entry_capacity(
    current_len_including_root: usize,
    max_entries: usize,
) -> Result<(), ContainerFilesystemError> {
    if current_len_including_root.saturating_sub(1) >= max_entries {
        Err(ContainerFilesystemError::EntryLimitExceeded { limit: max_entries })
    } else {
        Ok(())
    }
}

fn root_entry() -> ContainerFilesystemEntry {
    ContainerFilesystemEntry {
        raw_path: ".".to_string(),
        logical_path: "/".to_string(),
        name: "/".to_string(),
        display_name: "/".to_string(),
        entry_type: ContainerFilesystemEntryType::Directory,
        size: 0,
        mode: 0o755,
        uid: 0,
        gid: 0,
        mtime: None,
        link_target: None,
        origin: ContainerFilesystemOrigin::RootFilesystem,
    }
}

fn synthetic_directory(path: String) -> ContainerFilesystemEntry {
    let name = basename(&path).to_string();
    ContainerFilesystemEntry {
        raw_path: path.trim_start_matches('/').to_string(),
        logical_path: path,
        display_name: name.clone(),
        name,
        entry_type: ContainerFilesystemEntryType::Directory,
        size: 0,
        mode: 0,
        uid: 0,
        gid: 0,
        mtime: None,
        link_target: None,
        origin: ContainerFilesystemOrigin::SyntheticParent,
    }
}

fn apply_mount_overlays(
    snapshot: &mut ContainerFilesystemSnapshot,
    limits: &ContainerFilesystemLimits,
) -> Result<(), ContainerFilesystemError> {
    let mut known: HashSet<String> = snapshot
        .entries
        .iter()
        .map(|entry| entry.logical_path.clone())
        .collect();
    for overlay in &snapshot.mount_overlays {
        let mut missing = Vec::new();
        let mut current = overlay.destination.clone();
        while current != "/" {
            if !known.contains(&current) {
                missing.push(current.clone());
            }
            current = parent_path(&current);
        }
        missing.reverse();
        for path in missing {
            ensure_entry_capacity(snapshot.entries.len(), limits.max_entries)?;
            known.insert(path.clone());
            snapshot.entries.push(synthetic_directory(path));
        }
    }

    for entry in &mut snapshot.entries {
        if entry.logical_path == "/" {
            continue;
        }
        let exact = snapshot
            .mount_overlays
            .iter()
            .enumerate()
            .find(|(_, overlay)| overlay.destination == entry.logical_path)
            .map(|(index, _)| index);
        if let Some(mount_index) = exact {
            entry.origin = ContainerFilesystemOrigin::MountOverlay { mount_index };
            continue;
        }

        let route = snapshot
            .mount_overlays
            .iter()
            .enumerate()
            .filter(|(_, overlay)| is_descendant(&overlay.destination, &entry.logical_path))
            .max_by_key(|(_, overlay)| overlay.destination.len())
            .map(|(index, _)| index);
        if let Some(mount_index) = route {
            entry.origin = ContainerFilesystemOrigin::MountRoute { mount_index };
            continue;
        }

        let shadow = snapshot
            .mount_overlays
            .iter()
            .enumerate()
            .filter(|(_, overlay)| is_descendant(&entry.logical_path, &overlay.destination))
            .max_by_key(|(_, overlay)| overlay.destination.len())
            .map(|(index, _)| index);
        if let Some(mount_index) = shadow {
            entry.origin = ContainerFilesystemOrigin::ShadowedByMount { mount_index };
        }
    }
    Ok(())
}

async fn copy_single_file_archive<W: AsyncWrite + Unpin>(
    stream: ArchiveByteStream,
    requested: &str,
    writer: &mut W,
    limit: Option<u64>,
    limits: &ContainerFilesystemLimits,
) -> Result<ContainerFileTransfer, ContainerFilesystemError> {
    let mut reader = ArchiveReader::new(stream);
    let mut pending_pax = PaxValues::default();
    let mut global_pax = PaxValues::default();
    let mut pending_long_name = None;
    let mut seen_entry = false;
    let mut transfer = None;
    let mut zero_blocks = 0;

    loop {
        let block = reader.read_exact(TAR_BLOCK).await?;
        if block.iter().all(|byte| *byte == 0) {
            zero_blocks += 1;
            if zero_blocks == 2 {
                reader.ensure_zero_tail().await?;
                break;
            }
            continue;
        }
        if zero_blocks != 0 {
            return Err(ContainerFilesystemError::MalformedArchive(
                "non-zero header follows tar end marker".to_string(),
            ));
        }
        let header = parse_header(&block)?;
        match header.type_flag {
            b'x' | b'g' => {
                let data = read_extension(&mut reader, header.size, limits).await?;
                let pax = parse_pax(&data)?;
                if header.type_flag == b'g' {
                    global_pax.overlay(pax);
                } else {
                    pending_pax.overlay(pax);
                }
                continue;
            }
            b'L' => {
                let data = read_extension(&mut reader, header.size, limits).await?;
                pending_long_name = Some(parse_long_value(&data)?);
                continue;
            }
            b'K' => {
                return Err(ContainerFilesystemError::AmbiguousArchive(
                    "GNU long-link metadata is not valid for a regular-file download".to_string(),
                ));
            }
            _ => {}
        }
        if seen_entry {
            return Err(ContainerFilesystemError::AmbiguousArchive(
                "archive contains more than one entry".to_string(),
            ));
        }
        seen_entry = true;

        let mut pax = global_pax.clone();
        pax.overlay(std::mem::take(&mut pending_pax));
        if pax.link_path.is_some() || header.link_target.is_some() {
            return Err(ContainerFilesystemError::AmbiguousArchive(
                "download archive contains link metadata".to_string(),
            ));
        }
        let raw_path = pax
            .path
            .take()
            .or_else(|| pending_long_name.take())
            .unwrap_or(header.raw_path);
        let actual = normalize_tar_path(&raw_path, limits.max_path_bytes)?;
        if !archive_path_matches(requested, &actual) {
            return Err(ContainerFilesystemError::UnexpectedArchiveEntry {
                requested: requested.to_string(),
                actual,
            });
        }
        if header.entry_type != ContainerFilesystemEntryType::File
            || !matches!(header.type_flag, 0 | b'0' | b'7')
        {
            return Err(ContainerFilesystemError::NotRegularFile(
                requested.to_string(),
            ));
        }
        let file_size = pax.size.unwrap_or(header.size);
        let bytes_written = reader
            .copy_exact(writer, file_size, limit, requested)
            .await?;
        reader.skip_exact(padding(file_size)).await?;
        transfer = Some(ContainerFileTransfer {
            path: requested.to_string(),
            bytes_written,
            file_size,
        });
    }

    transfer.ok_or_else(|| {
        ContainerFilesystemError::AmbiguousArchive("archive contains no file entry".to_string())
    })
}

fn archive_path_matches(requested: &str, actual: &str) -> bool {
    actual == requested
        || (actual.matches('/').count() == 1 && basename(actual) == basename(requested))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[test]
    fn save_part_paths_are_unique_siblings() {
        let destination = Path::new("/tmp/report.txt");
        let first = part_path(destination).unwrap();
        let second = part_path(destination).unwrap();
        assert_eq!(first.parent(), destination.parent());
        assert_eq!(second.parent(), destination.parent());
        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".part")
        );
    }

    #[derive(Clone)]
    struct TestEntry<'a> {
        path: &'a str,
        kind: u8,
        data: &'a [u8],
        link: &'a str,
    }

    fn octal(field: &mut [u8], value: u64) {
        field.fill(0);
        let text = format!("{:0width$o}", value, width = field.len() - 1);
        field[..text.len()].copy_from_slice(text.as_bytes());
    }

    fn header(path: &str, kind: u8, size: u64, link: &str) -> [u8; TAR_BLOCK] {
        let mut block = [0u8; TAR_BLOCK];
        assert!(path.len() <= 100);
        block[..path.len()].copy_from_slice(path.as_bytes());
        octal(&mut block[100..108], 0o755);
        octal(&mut block[108..116], 1000);
        octal(&mut block[116..124], 1001);
        octal(&mut block[124..136], size);
        octal(&mut block[136..148], 1_700_000_000);
        block[148..156].fill(b' ');
        block[156] = kind;
        block[157..157 + link.len()].copy_from_slice(link.as_bytes());
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");
        let checksum: u64 = block.iter().map(|byte| *byte as u64).sum();
        let checksum_text = format!("{checksum:06o}\0 ");
        block[148..156].copy_from_slice(checksum_text.as_bytes());
        block
    }

    fn append_entry(tar: &mut Vec<u8>, entry: TestEntry<'_>) {
        tar.extend_from_slice(&header(
            entry.path,
            entry.kind,
            entry.data.len() as u64,
            entry.link,
        ));
        tar.extend_from_slice(entry.data);
        tar.resize(tar.len() + padding(entry.data.len() as u64) as usize, 0);
    }

    fn finish(tar: &mut Vec<u8>) {
        tar.resize(tar.len() + TAR_BLOCK * 2, 0);
    }

    fn tar(entries: &[TestEntry<'_>]) -> Vec<u8> {
        let mut tar = Vec::new();
        for entry in entries {
            append_entry(&mut tar, entry.clone());
        }
        finish(&mut tar);
        tar
    }

    fn pax_record(key: &str, value: &str) -> Vec<u8> {
        let body = format!("{key}={value}\n");
        let mut length = body.len() + 2;
        loop {
            let candidate = body.len() + length.to_string().len() + 1;
            if candidate == length {
                return format!("{length} {body}").into_bytes();
            }
            length = candidate;
        }
    }

    fn chunks(bytes: Vec<u8>, sizes: &[usize]) -> ArchiveByteStream {
        let mut output = Vec::new();
        let mut offset = 0;
        let mut size_index = 0;
        while offset < bytes.len() {
            let size = sizes[size_index % sizes.len()].max(1);
            let end = (offset + size).min(bytes.len());
            output.push(Ok(Bytes::copy_from_slice(&bytes[offset..end])));
            offset = end;
            size_index += 1;
        }
        Box::pin(stream::iter(output))
    }

    async fn snapshot(
        bytes: Vec<u8>,
        overlays: Vec<ContainerMountOverlay>,
        limits: ContainerFilesystemLimits,
    ) -> Result<ContainerFilesystemSnapshot, ContainerFilesystemError> {
        build_snapshot_from_stream(
            "container",
            overlays,
            DEFAULT_SNAPSHOT_TTL,
            limits,
            chunks(bytes, &[1, 7, 509, 3, 1024]),
        )
        .await
    }

    #[tokio::test]
    async fn indexes_root_dirs_files_links_hidden_across_chunks() {
        let archive = tar(&[
            TestEntry {
                path: ".",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "etc/",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "etc/hosts",
                kind: b'0',
                data: b"hello",
                link: "",
            },
            TestEntry {
                path: "etc/link",
                kind: b'2',
                data: b"",
                link: "hosts",
            },
            TestEntry {
                path: "etc/hard",
                kind: b'1',
                data: b"",
                link: "etc/hosts",
            },
            TestEntry {
                path: "etc/.secret",
                kind: b'0',
                data: b"x",
                link: "",
            },
        ]);
        let snapshot = snapshot(archive, vec![], ContainerFilesystemLimits::default())
            .await
            .unwrap();
        assert_eq!(snapshot.entries.len(), 6);
        assert_eq!(snapshot.entry("/etc/hosts").unwrap().unwrap().size, 5);
        assert_eq!(
            snapshot.entry("/etc/link").unwrap().unwrap().entry_type,
            ContainerFilesystemEntryType::Symlink
        );
        assert_eq!(
            snapshot.entry("/etc/hard").unwrap().unwrap().entry_type,
            ContainerFilesystemEntryType::Hardlink
        );
        let page = snapshot
            .list_directory(&ContainerDirectoryQuery {
                directory: "/etc".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.entries.len(), 3);
        assert!(page.entries.iter().all(|entry| entry.name != ".secret"));
    }

    #[tokio::test]
    async fn supports_pax_and_gnu_long_paths() {
        let pax_path = format!("deep/{}/file", "a".repeat(140));
        let pax = pax_record("path", &pax_path);
        let gnu_path = format!("gnu/{}/file", "b".repeat(130));
        let mut archive = Vec::new();
        append_entry(
            &mut archive,
            TestEntry {
                path: "PaxHeader",
                kind: b'x',
                data: &pax,
                link: "",
            },
        );
        append_entry(
            &mut archive,
            TestEntry {
                path: "short",
                kind: b'0',
                data: b"pax",
                link: "",
            },
        );
        let mut gnu_data = gnu_path.as_bytes().to_vec();
        gnu_data.push(0);
        append_entry(
            &mut archive,
            TestEntry {
                path: "LongLink",
                kind: b'L',
                data: &gnu_data,
                link: "",
            },
        );
        append_entry(
            &mut archive,
            TestEntry {
                path: "short2",
                kind: b'0',
                data: b"gnu",
                link: "",
            },
        );
        finish(&mut archive);
        let snapshot = snapshot(archive, vec![], ContainerFilesystemLimits::default())
            .await
            .unwrap();
        assert!(snapshot.entry(&format!("/{pax_path}")).unwrap().is_some());
        assert!(snapshot.entry(&format!("/{gnu_path}")).unwrap().is_some());
    }

    #[tokio::test]
    async fn rejects_traversal_absolute_nul_and_malformed_size() {
        for path in ["../escape", "/absolute"] {
            let error = snapshot(
                tar(&[TestEntry {
                    path,
                    kind: b'0',
                    data: b"",
                    link: "",
                }]),
                vec![],
                ContainerFilesystemLimits::default(),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                ContainerFilesystemError::InvalidPath { .. }
            ));
        }

        let mut nul_header = header("safe", b'0', 0, "");
        nul_header[5] = 0;
        nul_header[6] = b'x';
        nul_header[148..156].fill(b' ');
        let sum: u64 = nul_header.iter().map(|byte| *byte as u64).sum();
        nul_header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut archive = nul_header.to_vec();
        finish(&mut archive);
        assert!(matches!(
            snapshot(archive, vec![], ContainerFilesystemLimits::default())
                .await
                .unwrap_err(),
            ContainerFilesystemError::MalformedArchive(_)
        ));

        let mut bad = header("file", b'0', 0, "");
        bad[124..136].fill(b'9');
        bad[148..156].fill(b' ');
        let sum: u64 = bad.iter().map(|byte| *byte as u64).sum();
        bad[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut archive = bad.to_vec();
        finish(&mut archive);
        assert!(matches!(
            snapshot(archive, vec![], ContainerFilesystemLimits::default())
                .await
                .unwrap_err(),
            ContainerFilesystemError::MalformedArchive(_)
        ));
    }

    #[tokio::test]
    async fn rejects_truncated_stream_and_entry_cap() {
        let mut truncated = tar(&[TestEntry {
            path: "file",
            kind: b'0',
            data: b"payload",
            link: "",
        }]);
        truncated.truncate(TAR_BLOCK + 3);
        assert!(matches!(
            snapshot(truncated, vec![], ContainerFilesystemLimits::default())
                .await
                .unwrap_err(),
            ContainerFilesystemError::TruncatedArchive
        ));

        let mut trailing_data = tar(&[TestEntry {
            path: "file",
            kind: b'0',
            data: b"payload",
            link: "",
        }]);
        trailing_data.push(1);
        assert!(matches!(
            snapshot(trailing_data, vec![], ContainerFilesystemLimits::default())
                .await
                .unwrap_err(),
            ContainerFilesystemError::MalformedArchive(_)
        ));

        let limits = ContainerFilesystemLimits {
            max_entries: 1,
            ..Default::default()
        };
        let archive = tar(&[
            TestEntry {
                path: "one",
                kind: b'0',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "two",
                kind: b'0',
                data: b"",
                link: "",
            },
        ]);
        assert!(matches!(
            snapshot(archive, vec![], limits).await.unwrap_err(),
            ContainerFilesystemError::EntryLimitExceeded { limit: 1 }
        ));
    }

    #[tokio::test]
    async fn mount_overlay_marks_destination_hides_descendants_and_adds_missing_route() {
        let archive = tar(&[
            TestEntry {
                path: "var/",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "var/lib/",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "var/lib/data/",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "var/lib/data/image-only",
                kind: b'0',
                data: b"old",
                link: "",
            },
        ]);
        let overlays = vec![
            ContainerMountOverlay::new(
                ContainerMountOverlayKind::Volume,
                "/var/lib/data",
                Some("db".into()),
                false,
            )
            .unwrap(),
            ContainerMountOverlay::new(
                ContainerMountOverlayKind::Bind,
                "/workspace/src",
                Some("/host/src".into()),
                true,
            )
            .unwrap(),
        ];
        let snapshot = snapshot(archive, overlays, ContainerFilesystemLimits::default())
            .await
            .unwrap();
        assert!(matches!(
            snapshot.entry("/var/lib/data").unwrap().unwrap().origin,
            ContainerFilesystemOrigin::MountOverlay { mount_index: 0 }
        ));
        assert!(matches!(
            snapshot
                .entry("/var/lib/data/image-only")
                .unwrap()
                .unwrap()
                .origin,
            ContainerFilesystemOrigin::ShadowedByMount { mount_index: 0 }
        ));
        assert!(matches!(
            snapshot.entry("/workspace").unwrap().unwrap().origin,
            ContainerFilesystemOrigin::MountRoute { mount_index: 1 }
        ));
        assert!(matches!(
            snapshot.entry("/workspace/src").unwrap().unwrap().origin,
            ContainerFilesystemOrigin::MountOverlay { mount_index: 1 }
        ));
        let hidden = snapshot
            .list_directory(&ContainerDirectoryQuery {
                directory: "/var/lib/data".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(hidden.entries.is_empty());
        let visible = snapshot
            .list_directory(&ContainerDirectoryQuery {
                directory: "/var/lib/data".into(),
                include_shadowed: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(visible.entries.len(), 1);
    }

    #[tokio::test]
    async fn directory_sort_pagination_cursor_and_ttl_are_snapshot_scoped() {
        let archive = tar(&[
            TestEntry {
                path: "d/",
                kind: b'5',
                data: b"",
                link: "",
            },
            TestEntry {
                path: "d/c",
                kind: b'0',
                data: b"333",
                link: "",
            },
            TestEntry {
                path: "d/a",
                kind: b'0',
                data: b"1",
                link: "",
            },
            TestEntry {
                path: "d/b",
                kind: b'0',
                data: b"22",
                link: "",
            },
        ]);
        let mut snapshot = snapshot(archive, vec![], ContainerFilesystemLimits::default())
            .await
            .unwrap();
        snapshot.generated_at = Utc::now();
        snapshot.ttl = Duration::from_secs(10);
        assert!(snapshot.is_fresh_at(snapshot.generated_at + TimeDelta::seconds(10)));
        assert!(!snapshot.is_fresh_at(snapshot.generated_at + TimeDelta::seconds(11)));

        let first = snapshot
            .list_directory(&ContainerDirectoryQuery {
                directory: "/d".into(),
                sort: ContainerDirectorySort::Size,
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        let second = snapshot
            .list_directory(&ContainerDirectoryQuery {
                directory: "/d".into(),
                sort: ContainerDirectorySort::Size,
                limit: 2,
                cursor: first.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            second
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[tokio::test]
    async fn single_file_copy_is_bounded_and_rejects_ambiguity() {
        let archive = tar(&[TestEntry {
            path: "hosts",
            kind: b'0',
            data: b"123456",
            link: "",
        }]);
        let mut output = Vec::new();
        let transfer = copy_single_file_archive(
            chunks(archive, &[2, 511, 1, 3]),
            "/etc/hosts",
            &mut output,
            Some(3),
            &ContainerFilesystemLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(output, b"123");
        assert_eq!(transfer.file_size, 6);
        assert_eq!(transfer.bytes_written, 3);

        let archive = tar(&[
            TestEntry {
                path: "hosts",
                kind: b'0',
                data: b"a",
                link: "",
            },
            TestEntry {
                path: "other",
                kind: b'0',
                data: b"b",
                link: "",
            },
        ]);
        assert!(matches!(
            copy_single_file_archive(
                chunks(archive, &[13]),
                "/etc/hosts",
                &mut Vec::new(),
                None,
                &ContainerFilesystemLimits::default()
            )
            .await
            .unwrap_err(),
            ContainerFilesystemError::AmbiguousArchive(_)
        ));
    }

    #[tokio::test]
    async fn single_file_copy_rejects_traversal_symlink_and_wrong_name() {
        for (entry, expected) in [
            (
                TestEntry {
                    path: "../hosts",
                    kind: b'0',
                    data: b"x",
                    link: "",
                },
                "invalid",
            ),
            (
                TestEntry {
                    path: "hosts",
                    kind: b'2',
                    data: b"",
                    link: "target",
                },
                "regular",
            ),
            (
                TestEntry {
                    path: "passwd",
                    kind: b'0',
                    data: b"x",
                    link: "",
                },
                "unexpected",
            ),
        ] {
            let error = copy_single_file_archive(
                chunks(tar(&[entry]), &[37]),
                "/etc/hosts",
                &mut Vec::new(),
                None,
                &ContainerFilesystemLimits::default(),
            )
            .await
            .unwrap_err();
            match expected {
                "invalid" => assert!(matches!(
                    error,
                    ContainerFilesystemError::InvalidPath { .. }
                )),
                "regular" => assert!(matches!(
                    error,
                    ContainerFilesystemError::AmbiguousArchive(_)
                        | ContainerFilesystemError::NotRegularFile(_)
                )),
                _ => assert!(matches!(
                    error,
                    ContainerFilesystemError::UnexpectedArchiveEntry { .. }
                )),
            }
        }
    }

    #[tokio::test]
    async fn cancellation_and_timeout_are_non_blocking() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            run_cancellable(Duration::from_secs(1), &cancellation, async {
                Ok::<_, ContainerFilesystemError>(())
            })
            .await
            .unwrap_err(),
            ContainerFilesystemError::Cancelled
        ));
        assert!(matches!(
            run_cancellable(Duration::from_millis(1), &CancellationToken::new(), async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok::<_, ContainerFilesystemError>(())
            })
            .await
            .unwrap_err(),
            ContainerFilesystemError::Timeout
        ));
    }
}
