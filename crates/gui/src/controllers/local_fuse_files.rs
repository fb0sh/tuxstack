//! Pure state and asynchronous local-I/O layer for the unified FUSE browser.
//!
//! The daemon is authoritative only for mount/resource/provider information.
//! Once [`LocalFuseResourcePath`] has been installed by the bridge, directory
//! listings, metadata, previews, and Save As copies use the local FUSE path.
//! Raw Unix names remain byte vectors; QML receives reversible percent-encoded
//! tokens plus a deliberately lossy display label.

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub const PREVIEW_LIMIT: u64 = 1024 * 1024;
static PART_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LocalFuseResourceRef {
    Container(String),
    Image(String),
    Volume(String),
}

impl LocalFuseResourceRef {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Container(_) => "container",
            Self::Image(_) => "image",
            Self::Volume(_) => "volume",
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Container(value) | Self::Image(value) | Self::Volume(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalFuseFilesState {
    #[default]
    Idle,
    Resolving,
    Loading,
    Ready,
    Empty,
    DaemonOffline,
    FuseOffline,
    DockerOffline,
    ProviderUnavailable,
    PermissionDenied,
    IndexBuilding,
    SnapshotBuilding,
    Error,
}

impl LocalFuseFilesState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Resolving => "resolving",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::DaemonOffline => "daemon_offline",
            Self::FuseOffline => "fuse_offline",
            Self::DockerOffline => "docker_offline",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::IndexBuilding => "index_building",
            Self::SnapshotBuilding => "snapshot_building",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalFileSortColumn {
    #[default]
    Name,
    Modified,
    Size,
    Kind,
}

impl LocalFileSortColumn {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "modified" | "mtime" | "date" => Self::Modified,
            "size" => Self::Size,
            "kind" | "type" => Self::Kind,
            _ => Self::Name,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Kind => "kind",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalFileKind {
    Directory,
    RegularFile,
    Symlink,
    Socket,
    Fifo,
    BlockDevice,
    CharacterDevice,
    Unknown,
}

impl LocalFileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::RegularFile => "file",
            Self::Symlink => "symlink",
            Self::Socket => "socket",
            Self::Fifo => "fifo",
            Self::BlockDevice => "block_device",
            Self::CharacterDevice => "character_device",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_directory(self) -> bool {
        self == Self::Directory
    }

    pub fn is_previewable(self) -> bool {
        matches!(self, Self::RegularFile | Self::Symlink)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFileEntry {
    pub name_raw: Vec<u8>,
    pub display_name: String,
    pub path_components: Vec<Vec<u8>>,
    pub path_token: String,
    pub display_path: String,
    pub kind: LocalFileKind,
    pub size: u64,
    pub modified_unix_seconds: i64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub hidden: bool,
    pub symlink_target_raw: Option<Vec<u8>>,
    pub symlink_target_display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFuseResourcePath {
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breadcrumb {
    pub label: String,
    pub path_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewResult {
    pub bytes: Vec<u8>,
    pub file_size: u64,
    pub truncated: bool,
    pub mime_hint: String,
}

#[derive(Debug, Clone)]
pub struct LocalFuseFilesController {
    pub state: LocalFuseFilesState,
    pub active: bool,
    pub resource: Option<LocalFuseResourceRef>,
    pub resource_path: Option<LocalFuseResourcePath>,
    pub current_components: Vec<Vec<u8>>,
    pub history: Vec<Vec<Vec<u8>>>,
    pub show_hidden: bool,
    pub search_query: String,
    pub sort_column: LocalFileSortColumn,
    pub sort_descending: bool,
    pub entries: Vec<LocalFileEntry>,
    pub selected_token: Option<String>,
    pub error_kind: String,
    pub error_message: String,
    pub generation: u64,
}

impl Default for LocalFuseFilesController {
    fn default() -> Self {
        Self {
            state: LocalFuseFilesState::Idle,
            active: false,
            resource: None,
            resource_path: None,
            current_components: Vec::new(),
            history: Vec::new(),
            show_hidden: false,
            search_query: String::new(),
            sort_column: LocalFileSortColumn::Name,
            sort_descending: false,
            entries: Vec::new(),
            selected_token: None,
            error_kind: String::new(),
            error_message: String::new(),
            generation: 0,
        }
    }
}

impl LocalFuseFilesController {
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn select_resource(&mut self, resource: LocalFuseResourceRef) -> bool {
        if self.resource.as_ref() == Some(&resource) {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.resource = Some(resource);
        self.resource_path = None;
        self.current_components.clear();
        self.history.clear();
        self.search_query.clear();
        self.entries.clear();
        self.selected_token = None;
        self.clear_error();
        self.state = LocalFuseFilesState::Resolving;
        true
    }

    pub fn clear_resource(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.resource = None;
        self.resource_path = None;
        self.current_components.clear();
        self.history.clear();
        self.search_query.clear();
        self.entries.clear();
        self.selected_token = None;
        self.clear_error();
        self.state = LocalFuseFilesState::Idle;
    }

    pub fn begin_resolve(&mut self) -> Option<(u64, LocalFuseResourceRef)> {
        let resource = self.resource.clone()?;
        self.generation = self.generation.wrapping_add(1);
        self.clear_error();
        self.state = LocalFuseFilesState::Resolving;
        Some((self.generation, resource))
    }

    pub fn apply_resource_path(
        &mut self,
        generation: u64,
        resource_path: LocalFuseResourcePath,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.resource_path = Some(resource_path);
        self.state = LocalFuseFilesState::Loading;
        true
    }

    pub fn begin_list(&mut self) -> Option<(u64, PathBuf, Vec<Vec<u8>>)> {
        let root = self.resource_path.as_ref()?.root.clone();
        self.generation = self.generation.wrapping_add(1);
        self.clear_error();
        self.state = LocalFuseFilesState::Loading;
        Some((self.generation, root, self.current_components.clone()))
    }

    pub fn apply_entries(&mut self, generation: u64, entries: Vec<LocalFileEntry>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.entries = entries;
        self.selected_token = None;
        self.update_content_state();
        true
    }

    pub fn apply_error(
        &mut self,
        generation: u64,
        state: LocalFuseFilesState,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = state;
        self.error_kind = kind.into();
        self.error_message = message.into();
        self.entries.clear();
        self.selected_token = None;
        true
    }

    pub fn set_external_state(
        &mut self,
        state: LocalFuseFilesState,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.generation = self.generation.wrapping_add(1);
        self.state = state;
        self.error_kind = kind.into();
        self.error_message = message.into();
        self.entries.clear();
        self.selected_token = None;
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        !self.current_components.is_empty()
    }

    pub fn navigate_to_token(&mut self, token: &str, push_history: bool) -> bool {
        let Ok(components) = decode_path_token(token) else {
            return false;
        };
        if components == self.current_components {
            return false;
        }
        if push_history {
            self.history.push(self.current_components.clone());
        }
        self.current_components = components;
        self.search_query.clear();
        self.entries.clear();
        self.selected_token = None;
        true
    }

    pub fn go_back(&mut self) -> bool {
        let Some(previous) = self.history.pop() else {
            return false;
        };
        self.current_components = previous;
        self.search_query.clear();
        self.entries.clear();
        self.selected_token = None;
        true
    }

    pub fn go_up(&mut self) -> bool {
        if self.current_components.is_empty() {
            return false;
        }
        self.history.push(self.current_components.clone());
        self.current_components.pop();
        self.search_query.clear();
        self.entries.clear();
        self.selected_token = None;
        true
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        self.update_content_state();
    }

    pub fn set_search(&mut self, query: &str) {
        self.search_query = query.trim().to_string();
        self.update_content_state();
    }

    pub fn toggle_sort(&mut self, column: LocalFileSortColumn) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
    }

    pub fn select_token(&mut self, token: Option<&str>) {
        self.selected_token = token
            .filter(|value| self.entries.iter().any(|entry| entry.path_token == *value))
            .map(str::to_owned);
    }

    pub fn entry(&self, token: &str) -> Option<&LocalFileEntry> {
        self.entries.iter().find(|entry| entry.path_token == token)
    }

    pub fn visible_entries(&self) -> Vec<&LocalFileEntry> {
        let query = self.search_query.to_lowercase();
        let mut visible = self
            .entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.hidden)
            .filter(|entry| {
                query.is_empty()
                    || entry.display_name.to_lowercase().contains(&query)
                    || entry.display_path.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        visible.sort_by(|left, right| self.compare_entries(left, right));
        visible
    }

    pub fn current_display_path(&self) -> String {
        display_path(&self.current_components)
    }

    pub fn breadcrumbs(&self) -> Vec<Breadcrumb> {
        let root_label = self
            .resource
            .as_ref()
            .map(|resource| resource.id().to_string())
            .unwrap_or_else(|| "/".into());
        let mut result = vec![Breadcrumb {
            label: root_label,
            path_token: "/".into(),
        }];
        let mut components = Vec::new();
        for component in &self.current_components {
            components.push(component.clone());
            result.push(Breadcrumb {
                label: String::from_utf8_lossy(component).into_owned(),
                path_token: encode_path_token(&components),
            });
        }
        result
    }

    pub fn local_path_for_components(&self, components: &[Vec<u8>]) -> Option<PathBuf> {
        let root = &self.resource_path.as_ref()?.root;
        join_validated_components(root, components).ok()
    }

    pub fn local_path_for_token(&self, token: &str) -> Option<PathBuf> {
        let components = decode_path_token(token).ok()?;
        self.local_path_for_components(&components)
    }

    fn compare_entries(&self, left: &LocalFileEntry, right: &LocalFileEntry) -> Ordering {
        let directory_order = right.kind.is_directory().cmp(&left.kind.is_directory());
        if directory_order != Ordering::Equal {
            return directory_order;
        }
        let primary = match self.sort_column {
            LocalFileSortColumn::Name => compare_names(left, right),
            LocalFileSortColumn::Modified => {
                left.modified_unix_seconds.cmp(&right.modified_unix_seconds)
            }
            LocalFileSortColumn::Size => left.size.cmp(&right.size),
            LocalFileSortColumn::Kind => left.kind.cmp(&right.kind),
        };
        let ordering = primary.then_with(|| compare_names(left, right));
        if self.sort_descending {
            ordering.reverse()
        } else {
            ordering
        }
    }

    fn update_content_state(&mut self) {
        if matches!(
            self.state,
            LocalFuseFilesState::Ready | LocalFuseFilesState::Empty
        ) {
            self.state = if self.visible_entries().is_empty() {
                LocalFuseFilesState::Empty
            } else {
                LocalFuseFilesState::Ready
            };
        }
    }

    fn clear_error(&mut self) {
        self.error_kind.clear();
        self.error_message.clear();
    }
}

/// Read one local FUSE directory without converting names through UTF-8.
pub async fn read_local_directory(
    root: PathBuf,
    components: Vec<Vec<u8>>,
) -> io::Result<Vec<LocalFileEntry>> {
    let directory = join_validated_components(&root, &components)?;
    let mut reader = tokio::fs::read_dir(directory).await?;
    let mut entries = Vec::new();
    while let Some(item) = reader.next_entry().await? {
        let name = item.file_name();
        let raw = name.as_bytes().to_vec();
        if !valid_component(&raw) {
            continue;
        }
        let mut path_components = components.clone();
        path_components.push(raw.clone());
        let path = item.path();
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        let file_type = metadata.file_type();
        let kind = local_kind(&file_type);
        let symlink_target_raw = if kind == LocalFileKind::Symlink {
            tokio::fs::read_link(&path)
                .await
                .ok()
                .map(|target| target.as_os_str().as_bytes().to_vec())
        } else {
            None
        };
        entries.push(LocalFileEntry {
            name_raw: raw.clone(),
            display_name: String::from_utf8_lossy(&raw).into_owned(),
            path_token: encode_path_token(&path_components),
            display_path: display_path(&path_components),
            path_components,
            kind,
            size: metadata.len(),
            modified_unix_seconds: metadata.mtime(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            hidden: raw.first() == Some(&b'.'),
            symlink_target_display: symlink_target_raw
                .as_deref()
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .unwrap_or_default(),
            symlink_target_raw,
        });
    }
    Ok(entries)
}

/// Read at most [`PREVIEW_LIMIT`] bytes through the local FUSE mount.
pub async fn preview_local_file(path: PathBuf, name_raw: &[u8]) -> io::Result<PreviewResult> {
    let file = tokio::fs::File::open(&path).await?;
    let metadata = file.metadata().await?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only regular files can be previewed",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(PREVIEW_LIMIT) as usize);
    file.take(PREVIEW_LIMIT.saturating_add(1))
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() as u64 > PREVIEW_LIMIT;
    bytes.truncate(PREVIEW_LIMIT as usize);
    Ok(PreviewResult {
        bytes,
        file_size: metadata.len(),
        truncated,
        mime_hint: mime_hint(name_raw).into(),
    })
}

/// Copy a local FUSE file to a unique `.part` sibling and atomically rename it.
/// No partial destination is ever exposed, and an error removes the part file.
pub async fn copy_local_file_atomic_cancellable(
    source: PathBuf,
    destination: PathBuf,
    cancellation: CancellationToken,
) -> io::Result<u64> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination has no parent folder",
        )
    })?;
    if !tokio::fs::metadata(parent).await?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination parent is not a folder",
        ));
    }
    let part = unique_part_path(&destination);
    let result = async {
        let mut input = tokio::fs::File::open(&source).await?;
        if !input.metadata().await?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only regular files can be saved",
            ));
        }
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .await?;
        let mut copied = 0_u64;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            let count = tokio::select! {
                _ = cancellation.cancelled() => {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "save cancelled"));
                }
                result = input.read(&mut buffer) => result?,
            };
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).await?;
            copied = copied.saturating_add(count as u64);
        }
        output.flush().await?;
        output.sync_all().await?;
        drop(output);
        if cancellation.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "save cancelled"));
        }
        tokio::fs::rename(&part, &destination).await?;
        Ok(copied)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    result
}

pub fn encode_path_token(components: &[Vec<u8>]) -> String {
    if components.is_empty() {
        return "/".into();
    }
    let mut result = String::new();
    for component in components {
        result.push('/');
        for byte in component {
            result.push('%');
            result.push(hex_digit(byte >> 4));
            result.push(hex_digit(byte & 0x0f));
        }
    }
    result
}

pub fn decode_path_token(token: &str) -> Result<Vec<Vec<u8>>, &'static str> {
    if token == "/" {
        return Ok(Vec::new());
    }
    if !token.starts_with('/') || token.ends_with('/') {
        return Err("invalid path token");
    }
    token[1..]
        .split('/')
        .map(|component| {
            let bytes = component.as_bytes();
            if bytes.is_empty() || bytes.len() % 3 != 0 {
                return Err("invalid path token component");
            }
            let mut decoded = Vec::with_capacity(bytes.len() / 3);
            for chunk in bytes.chunks_exact(3) {
                if chunk[0] != b'%' {
                    return Err("path tokens must percent-encode every byte");
                }
                let high = hex_value(chunk[1]).ok_or("invalid path token escape")?;
                let low = hex_value(chunk[2]).ok_or("invalid path token escape")?;
                decoded.push((high << 4) | low);
            }
            if valid_component(&decoded) {
                Ok(decoded)
            } else {
                Err("invalid path component")
            }
        })
        .collect()
}

fn join_validated_components(root: &Path, components: &[Vec<u8>]) -> io::Result<PathBuf> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon resource path is not absolute",
        ));
    }
    let mut path = root.to_path_buf();
    for component in components {
        if !valid_component(component) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid local FUSE path component",
            ));
        }
        path.push(OsString::from_vec(component.clone()));
    }
    Ok(path)
}

fn valid_component(value: &[u8]) -> bool {
    !value.is_empty()
        && value != b"."
        && value != b".."
        && !value.contains(&0)
        && !value.contains(&b'/')
}

fn local_kind(file_type: &std::fs::FileType) -> LocalFileKind {
    if file_type.is_dir() {
        LocalFileKind::Directory
    } else if file_type.is_file() {
        LocalFileKind::RegularFile
    } else if file_type.is_symlink() {
        LocalFileKind::Symlink
    } else if file_type.is_socket() {
        LocalFileKind::Socket
    } else if file_type.is_fifo() {
        LocalFileKind::Fifo
    } else if file_type.is_block_device() {
        LocalFileKind::BlockDevice
    } else if file_type.is_char_device() {
        LocalFileKind::CharacterDevice
    } else {
        LocalFileKind::Unknown
    }
}

fn compare_names(left: &LocalFileEntry, right: &LocalFileEntry) -> Ordering {
    left.display_name
        .to_lowercase()
        .cmp(&right.display_name.to_lowercase())
        .then_with(|| left.name_raw.cmp(&right.name_raw))
}

fn display_path(components: &[Vec<u8>]) -> String {
    if components.is_empty() {
        return "/".into();
    }
    let mut result = String::new();
    for component in components {
        result.push('/');
        result.push_str(&String::from_utf8_lossy(component));
    }
    result
}

fn mime_hint(name: &[u8]) -> &'static str {
    let lower = String::from_utf8_lossy(name).to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt")
        || lower.ends_with(".md")
        || lower.ends_with(".log")
        || lower.ends_with(".conf")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".xml")
        || lower.ends_with(".csv")
    {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

fn unique_part_path(destination: &Path) -> PathBuf {
    let sequence = PART_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut name = destination
        .file_name()
        .unwrap_or_else(|| OsStr::new("tuxstack-save"))
        .as_bytes()
        .to_vec();
    name.extend_from_slice(
        format!(".tuxstack-{}-{nanos:x}-{sequence}.part", std::process::id()).as_bytes(),
    );
    destination.with_file_name(OsString::from_vec(name))
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tuxstack-local-fuse-{label}-{}-{}",
            std::process::id(),
            PART_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed)
        ))
    }

    #[test]
    fn raw_non_utf8_tokens_round_trip_without_guessing_names() {
        let components = vec![b"usr".to_vec(), vec![b'f', 0x80, b'o']];
        let token = encode_path_token(&components);
        assert_eq!(token, "/%75%73%72/%66%80%6F");
        assert_eq!(decode_path_token(&token).unwrap(), components);
    }

    #[test]
    fn path_tokens_reject_traversal_nul_and_ambiguous_plain_text() {
        for token in ["/%2E%2E", "/%00", "/plain", "relative", "/%2F"] {
            assert!(decode_path_token(token).is_err(), "{token}");
        }
    }

    #[test]
    fn navigation_and_breadcrumbs_are_unified() {
        let mut state = LocalFuseFilesController::default();
        state.select_resource(LocalFuseResourceRef::Volume("data".into()));
        state.current_components = vec![b"one".to_vec(), b"two".to_vec()];
        assert!(state.can_go_up());
        assert_eq!(state.breadcrumbs().len(), 3);
        assert!(state.go_up());
        assert_eq!(state.current_display_path(), "/one");
        assert!(state.go_back());
        assert_eq!(state.current_display_path(), "/one/two");
    }

    #[test]
    fn search_hidden_and_sort_are_local_and_directories_stay_first() {
        fn entry(name: &str, kind: LocalFileKind, size: u64) -> LocalFileEntry {
            LocalFileEntry {
                name_raw: name.as_bytes().to_vec(),
                display_name: name.into(),
                path_components: vec![name.as_bytes().to_vec()],
                path_token: encode_path_token(&[name.as_bytes().to_vec()]),
                display_path: format!("/{name}"),
                kind,
                size,
                modified_unix_seconds: size as i64,
                mode: 0o444,
                uid: 1000,
                gid: 1000,
                hidden: name.starts_with('.'),
                symlink_target_raw: None,
                symlink_target_display: String::new(),
            }
        }

        let mut state = LocalFuseFilesController {
            state: LocalFuseFilesState::Ready,
            entries: vec![
                entry("large", LocalFileKind::RegularFile, 20),
                entry("folder", LocalFileKind::Directory, 0),
                entry("small", LocalFileKind::RegularFile, 2),
                entry(".secret", LocalFileKind::RegularFile, 1),
            ],
            ..Default::default()
        };
        state.toggle_sort(LocalFileSortColumn::Size);
        let names = state
            .visible_entries()
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["folder", "small", "large"]);

        state.set_show_hidden(true);
        state.set_search("secret");
        assert_eq!(state.visible_entries()[0].display_name, ".secret");
    }

    #[test]
    fn stale_async_results_cannot_replace_a_new_resource() {
        let mut state = LocalFuseFilesController::default();
        state.select_resource(LocalFuseResourceRef::Container("one".into()));
        let stale = state.begin_resolve().unwrap().0;
        state.select_resource(LocalFuseResourceRef::Image("two".into()));
        assert!(!state.apply_resource_path(
            stale,
            LocalFuseResourcePath {
                root: PathBuf::from("/tmp/stale")
            }
        ));
    }

    #[tokio::test]
    async fn directory_io_preserves_non_utf8_names_and_hidden_state() {
        let root = test_dir("bytes");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let raw_name = OsString::from_vec(vec![b'.', b'x', 0x80]);
        tokio::fs::write(root.join(&raw_name), b"value")
            .await
            .unwrap();
        let rows = read_local_directory(root.clone(), vec![]).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name_raw, vec![b'.', b'x', 0x80]);
        assert!(rows[0].hidden);
        assert_eq!(
            decode_path_token(&rows[0].path_token).unwrap()[0],
            rows[0].name_raw
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn preview_is_bounded_and_save_as_uses_atomic_final_name() {
        let root = test_dir("copy");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source.txt");
        let destination = root.join("saved.txt");
        tokio::fs::write(&source, b"hello fuse").await.unwrap();
        let preview = preview_local_file(source.clone(), b"source.txt")
            .await
            .unwrap();
        assert_eq!(preview.bytes, b"hello fuse");
        assert!(!preview.truncated);
        assert_eq!(
            copy_local_file_atomic_cancellable(
                source,
                destination.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap(),
            10
        );
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"hello fuse");
        let mut reader = tokio::fs::read_dir(&root).await.unwrap();
        while let Some(entry) = reader.next_entry().await.unwrap() {
            assert!(!entry.file_name().as_bytes().ends_with(b".part"));
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_save_removes_part_and_never_exposes_destination() {
        let root = test_dir("cancel-copy");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source.bin");
        let destination = root.join("saved.bin");
        tokio::fs::write(&source, [7_u8; 16]).await.unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result =
            copy_local_file_atomic_cancellable(source, destination.clone(), cancellation).await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!destination.exists());
        let mut reader = tokio::fs::read_dir(&root).await.unwrap();
        while let Some(entry) = reader.next_entry().await.unwrap() {
            assert!(!entry.file_name().as_bytes().ends_with(b".part"));
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
