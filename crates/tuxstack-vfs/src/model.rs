use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::{VirtualFileName, VirtualPath};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImagePlatform {
    pub architecture: String,
    pub operating_system: String,
    pub variant: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum DockerFilesystemResource {
    Container {
        container_id: String,
    },
    Image {
        image_id: String,
        platform: ImagePlatform,
    },
    Volume {
        volume_name: String,
    },
}

impl DockerFilesystemResource {
    pub fn canonical_id(&self) -> &str {
        match self {
            Self::Container { container_id } => container_id,
            Self::Image { image_id, .. } => image_id,
            Self::Volume { volume_name } => volume_name,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Container { .. } => "container",
            Self::Image { .. } => "image",
            Self::Volume { .. } => "volume",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProviderKind {
    ContainerRootfsSnapshot,
    ContainerArchiveLive,
    NamedVolumeLive,
    LocalBindLive,
    HelperBindLive,
    TmpfsLive,
    RuntimeMount,
    ImageRootfsImmutable,
    InMemory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsistencyMode {
    Immutable,
    Live,
    Snapshot {
        captured_at: DateTime<Utc>,
        generation: u64,
    },
    OperationTimeRead,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ProviderCapabilities(u32);

impl ProviderCapabilities {
    pub const LOOKUP: Self = Self(1 << 0);
    pub const READDIR: Self = Self(1 << 1);
    pub const GETATTR: Self = Self(1 << 2);
    pub const READLINK: Self = Self(1 << 3);
    pub const OPEN: Self = Self(1 << 4);
    pub const READ: Self = Self(1 << 5);
    pub const DOWNLOAD: Self = Self(1 << 6);
    pub const REFRESH: Self = Self(1 << 7);
    pub const READ_ONLY: Self = Self(
        Self::LOOKUP.0
            | Self::READDIR.0
            | Self::GETATTR.0
            | Self::READLINK.0
            | Self::OPEN.0
            | Self::READ.0
            | Self::REFRESH.0,
    );

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ProviderCapabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDescriptor {
    pub kind: ProviderKind,
    pub consistency: ConsistencyMode,
    pub source: Option<String>,
    pub capabilities: ProviderCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestContext {
    pub uid: u32,
    pub gid: u32,
    pub pid: u32,
    pub request_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum VirtualFileType {
    RegularFile,
    Directory,
    Symlink,
    CharacterDevice,
    BlockDevice,
    NamedPipe,
    Socket,
}

impl VirtualFileType {
    pub fn is_special(self) -> bool {
        matches!(
            self,
            Self::CharacterDevice | Self::BlockDevice | Self::NamedPipe | Self::Socket
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginalMetadata {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualMetadata {
    /// Provider-stable identity. Equal values model a hardlink.
    pub node_id: Vec<u8>,
    pub file_type: VirtualFileType,
    pub size: u64,
    pub nlink: u32,
    pub mtime: SystemTime,
    pub original: OriginalMetadata,
    pub device_id: Option<u64>,
    pub generation: u64,
}

impl VirtualMetadata {
    pub fn directory(node_id: impl Into<Vec<u8>>) -> Self {
        Self::new(node_id, VirtualFileType::Directory, 0)
    }

    pub fn file(node_id: impl Into<Vec<u8>>, size: u64) -> Self {
        Self::new(node_id, VirtualFileType::RegularFile, size)
    }

    pub fn symlink(node_id: impl Into<Vec<u8>>, size: u64) -> Self {
        Self::new(node_id, VirtualFileType::Symlink, size)
    }

    pub fn special(
        node_id: impl Into<Vec<u8>>,
        file_type: VirtualFileType,
        device_id: u64,
    ) -> Self {
        let mut metadata = Self::new(node_id, file_type, 0);
        metadata.device_id = Some(device_id);
        metadata
    }

    fn new(node_id: impl Into<Vec<u8>>, file_type: VirtualFileType, size: u64) -> Self {
        Self {
            node_id: node_id.into(),
            file_type,
            size,
            nlink: 1,
            mtime: SystemTime::UNIX_EPOCH,
            original: OriginalMetadata {
                mode: match file_type {
                    VirtualFileType::Directory => 0o755,
                    VirtualFileType::Symlink => 0o777,
                    _ => 0o644,
                },
                uid: 0,
                gid: 0,
            },
            device_id: None,
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualDirectoryEntry {
    pub name: VirtualFileName,
    pub metadata: VirtualMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProviderFileHandle {
    pub id: u64,
    pub path: VirtualPath,
    pub content_generation: u64,
}
