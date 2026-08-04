//! Domain models for read-only Docker volume file browsing.

use chrono::{DateTime, Utc};

/// Kind of filesystem entry inside a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeFileType {
    Directory,
    RegularFile,
    SymbolicLink,
    Socket,
    Fifo,
    BlockDevice,
    CharacterDevice,
    Unknown,
}

impl VolumeFileType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::RegularFile => "file",
            Self::SymbolicLink => "symlink",
            Self::Socket => "socket",
            Self::Fifo => "fifo",
            Self::BlockDevice => "block",
            Self::CharacterDevice => "char",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_protocol(code: &str) -> Self {
        match code {
            "d" | "directory" | "dir" => Self::Directory,
            "f" | "file" | "regular" => Self::RegularFile,
            "l" | "symlink" | "link" => Self::SymbolicLink,
            "s" | "socket" => Self::Socket,
            "p" | "fifo" | "pipe" => Self::Fifo,
            "b" | "block" => Self::BlockDevice,
            "c" | "char" => Self::CharacterDevice,
            _ => Self::Unknown,
        }
    }

    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }
}

/// Validated logical path inside a volume (`/` root, no `..`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct VolumePath {
    components: Vec<String>,
}

impl VolumePath {
    pub fn root() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    /// Parse a logical volume path such as `/`, `/dir`, or `dir/sub`.
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.contains('\0') {
            return Err("path contains a NUL byte".into());
        }
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed == "/" {
            return Ok(Self::root());
        }
        let without_root = trimmed.trim_start_matches('/');
        let mut components = Vec::new();
        for part in without_root.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err("path must not contain '..'".into());
            }
            if part.contains('\0') {
                return Err("path component contains a NUL byte".into());
            }
            components.push(part.to_string());
        }
        Ok(Self { components })
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn join_name(&self, name: &str) -> Result<Self, String> {
        if name.is_empty() || name == "." {
            return Ok(self.clone());
        }
        if name == ".." || name.contains('/') || name.contains('\0') {
            return Err("invalid path component".into());
        }
        let mut child = self.clone();
        child.components.push(name.to_string());
        Ok(child)
    }

    pub fn parent(&self) -> Option<Self> {
        if self.components.is_empty() {
            None
        } else {
            let mut parent = self.clone();
            parent.components.pop();
            Some(parent)
        }
    }

    /// Logical display path always starting with `/`.
    pub fn display(&self) -> String {
        if self.components.is_empty() {
            "/".into()
        } else {
            format!("/{}", self.components.join("/"))
        }
    }

    /// Absolute helper path under `/volume`.
    pub fn helper_absolute(&self) -> String {
        if self.components.is_empty() {
            "/volume".into()
        } else {
            format!("/volume/{}", self.components.join("/"))
        }
    }

    /// True when `other` is this path or a descendant of it.
    pub fn contains_path(&self, other: &Self) -> bool {
        other.components.starts_with(&self.components)
    }
}

/// One directory entry returned by a list operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeFileEntry {
    pub name: String,
    pub path: VolumePath,
    pub entry_type: VolumeFileType,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub symlink_target: Option<String>,
    pub mime_type: Option<String>,
    pub hidden: bool,
    pub readable: bool,
}

/// File properties for the properties dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeFileProperties {
    pub name: String,
    pub path: VolumePath,
    pub entry_type: VolumeFileType,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub symlink_target: Option<String>,
}

/// Kind of bounded file preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePreviewKind {
    Text,
    Json,
    Image,
    Binary,
    Unsupported,
}

/// Preview payload after bounded read + classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePreviewContent {
    Text(String),
    Json {
        pretty: Option<String>,
        raw: String,
        parse_error: Option<String>,
    },
    ImageBytes(Vec<u8>),
    BinaryInfo,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeFilePreview {
    pub path: VolumePath,
    pub name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub preview_kind: FilePreviewKind,
    pub content: FilePreviewContent,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct PreviewVolumeFileRequest {
    pub volume_name: String,
    pub path: VolumePath,
    pub max_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct DownloadVolumeFileRequest {
    pub volume_name: String,
    pub path: VolumePath,
    pub destination: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct ListVolumeDirectoryRequest {
    pub volume_name: String,
    pub path: VolumePath,
    pub show_hidden: bool,
}

/// Live helper session for one volume.
#[derive(Debug, Clone)]
pub struct VolumePreviewSession {
    pub id: uuid::Uuid,
    pub volume_name: String,
    pub container_id: String,
    pub container_name: String,
    pub started_at: DateTime<Utc>,
}

/// Helper container configuration.
#[derive(Debug, Clone)]
pub struct VolumeHelperConfig {
    pub image: String,
    pub mount_path: String,
    pub memory_limit_bytes: i64,
    pub nano_cpus: i64,
    pub pids_limit: i64,
    pub operation_timeout: std::time::Duration,
    pub text_preview_max_bytes: u64,
    pub image_preview_max_bytes: u64,
    pub max_directory_entries: usize,
}

impl Default for VolumeHelperConfig {
    fn default() -> Self {
        Self {
            image: "alpine:3.20".into(),
            mount_path: "/volume".into(),
            memory_limit_bytes: 128 * 1024 * 1024,
            nano_cpus: 250_000_000,
            pids_limit: 64,
            operation_timeout: std::time::Duration::from_secs(60),
            text_preview_max_bytes: 1024 * 1024,
            image_preview_max_bytes: 16 * 1024 * 1024,
            max_directory_entries: 50_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rejects_parent_and_nul() {
        assert!(VolumePath::parse("../etc").is_err());
        assert!(VolumePath::parse("/a/\0/b").is_err());
        assert!(VolumePath::parse("/a/../b").is_err());
    }

    #[test]
    fn path_normalizes_dots_and_slashes() {
        let path = VolumePath::parse("///a//./b///").unwrap();
        assert_eq!(path.display(), "/a/b");
        assert_eq!(path.helper_absolute(), "/volume/a/b");
        assert_eq!(path.parent().unwrap().display(), "/a");
        assert!(VolumePath::root().parent().is_none());
    }

    #[test]
    fn join_name_rejects_traversal() {
        let root = VolumePath::root();
        assert!(root.join_name("..").is_err());
        assert!(root.join_name("a/b").is_err());
        assert_eq!(root.join_name("ok").unwrap().display(), "/ok");
    }
}
