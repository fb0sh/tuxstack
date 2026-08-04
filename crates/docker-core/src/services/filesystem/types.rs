//! Unified types for the filesystem browsing service.
//!
//! These types replace the image-specific `ImagePreviewSession` /
//! `VolumePreviewSession` / `VolumeFileEntry` with a common representation
//! that both image and volume browsing share.

use chrono::{DateTime, Utc};

use tuxstack_fs_protocol::{FilesystemPathToken, encode_base64};

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

/// What we are browsing: either a Docker image or a mounted volume.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FilesystemSource {
    Image {
        image_id: String,
        platform: String,
    },
    Volume {
        volume_name: String,
    },
}

impl std::fmt::Display for FilesystemSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image { image_id, .. } => write!(f, "image:{image_id}"),
            Self::Volume { volume_name } => write!(f, "volume:{volume_name}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A live helper container session, shared by both image and volume browsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemSession {
    pub container_id: String,
    pub container_name: String,
    pub source: FilesystemSource,
    pub root: String,
    pub helper_path: String,
    pub protocol_version: u32,
    pub helper_version: String,
    pub read_only: bool,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/// Kind of filesystem entry in the browsing protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilesystemEntryType {
    Directory,
    RegularFile,
    SymbolicLink,
    Socket,
    Fifo,
    BlockDevice,
    CharacterDevice,
    Unknown,
}

impl FilesystemEntryType {
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

    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }

    /// Map from the protocol's `HelperFileType` to the domain enum.
    pub fn from_protocol(ft: tuxstack_fs_protocol::HelperFileType) -> Self {
        match ft {
            tuxstack_fs_protocol::HelperFileType::Directory => Self::Directory,
            tuxstack_fs_protocol::HelperFileType::File => Self::RegularFile,
            tuxstack_fs_protocol::HelperFileType::Symlink => Self::SymbolicLink,
            tuxstack_fs_protocol::HelperFileType::Socket => Self::Socket,
            tuxstack_fs_protocol::HelperFileType::Fifo => Self::Fifo,
            tuxstack_fs_protocol::HelperFileType::BlockDevice => Self::BlockDevice,
            tuxstack_fs_protocol::HelperFileType::CharacterDevice => Self::CharacterDevice,
            tuxstack_fs_protocol::HelperFileType::Unknown => Self::Unknown,
        }
    }

    pub fn from_protocol_str(code: &str) -> Self {
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
}

/// One directory entry returned by a list operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemEntry {
    /// Raw bytes of the entry name (may contain non-UTF-8 data).
    pub name_raw: Vec<u8>,
    /// Lossy display name for UI display.
    pub display_name: String,
    /// Opaque path token for this entry.
    pub path_token: FilesystemPathToken,
    pub entry_type: FilesystemEntryType,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub symlink_target_raw: Option<Vec<u8>>,
    pub symlink_target_display: Option<String>,
    pub readable: bool,
    pub hidden: bool,
}

impl FilesystemEntry {
    /// Build from protocol `Entry` message + parent token.
    pub fn from_protocol_entry(
        msg: &tuxstack_fs_protocol::HelperMessage,
    ) -> Option<Self> {
        if let tuxstack_fs_protocol::HelperMessage::Entry {
            name_b64,
            path_token,
            file_type,
            size,
            mtime,
            mode,
            uid,
            gid,
            symlink_target_b64,
            readable,
        } = msg
        {
            let name_raw = encode_base64_to_raw(name_b64)?;
            let display_name = String::from_utf8_lossy(&name_raw).into_owned();
            let hidden = name_raw.first() == Some(&b'.');
            let symlink_target_raw = symlink_target_b64.as_ref().and_then(|b| {
                tuxstack_fs_protocol::decode_base64(b).ok()
            });
            let symlink_target_display =
                symlink_target_raw.as_ref().map(|raw| String::from_utf8_lossy(raw).into_owned());
            Some(Self {
                name_raw,
                display_name,
                path_token: FilesystemPathToken(path_token.clone()),
                entry_type: FilesystemEntryType::from_protocol(*file_type),
                size_bytes: *size,
                modified_at: mtime.and_then(|ts| {
                    chrono::TimeZone::timestamp_opt(&chrono::Utc, ts, 0).single()
                }),
                mode: *mode,
                uid: *uid,
                gid: *gid,
                symlink_target_raw,
                symlink_target_display,
                readable: *readable,
                hidden,
            })
        } else {
            None
        }
    }

    /// Legacy compatibility: returns a `String` path display for callers that
    /// still need it (e.g. the GUI bridge). Prefer `path_token` going forward.
    pub fn path_display(&self) -> String {
        if self.path_token.is_root() {
            "/".into()
        } else {
            format!("/{}", self.path_token.as_str())
        }
    }
}

fn encode_base64_to_raw(b64: &str) -> Option<Vec<u8>> {
    tuxstack_fs_protocol::decode_base64(b64).ok()
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ListDirectoryRequest {
    pub path_token: FilesystemPathToken,
    pub show_hidden: bool,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatRequest {
    pub path_token: FilesystemPathToken,
}

#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub path_token: FilesystemPathToken,
    pub offset: u64,
    pub limit: u64,
}

#[derive(Debug, Clone)]
pub struct HashRequest {
    pub path_token: FilesystemPathToken,
    pub algorithm: String,
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ListDirectoryResult {
    pub entries: Vec<FilesystemEntry>,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub chunks: Vec<PreviewChunk>,
    pub total_length: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct PreviewChunk {
    pub data_b64: String,
    pub offset: u64,
    pub eof: bool,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_from_protocol_roundtrip() {
        let msg = tuxstack_fs_protocol::HelperMessage::Entry {
            name_b64: tuxstack_fs_protocol::encode_base64(b"test.txt"),
            path_token: FilesystemPathToken::from_relative("test.txt").unwrap().0,
            file_type: tuxstack_fs_protocol::HelperFileType::File,
            size: Some(42),
            mtime: Some(1_722_000_000),
            mode: Some(0o644),
            uid: Some(1000),
            gid: Some(1000),
            symlink_target_b64: None,
            readable: true,
        };
        let entry = FilesystemEntry::from_protocol_entry(&msg).unwrap();
        assert_eq!(entry.name_raw, b"test.txt");
        assert_eq!(entry.display_name, "test.txt");
        assert_eq!(entry.entry_type, FilesystemEntryType::RegularFile);
        assert_eq!(entry.size_bytes, Some(42));
        assert!(!entry.hidden);
    }

    #[test]
    fn entry_from_protocol_symlink() {
        let msg = tuxstack_fs_protocol::HelperMessage::Entry {
            name_b64: tuxstack_fs_protocol::encode_base64(b"link"),
            path_token: FilesystemPathToken::from_relative("link").unwrap().0,
            file_type: tuxstack_fs_protocol::HelperFileType::Symlink,
            size: None,
            mtime: None,
            mode: None,
            uid: None,
            gid: None,
            symlink_target_b64: Some(tuxstack_fs_protocol::encode_base64(b"target")),
            readable: true,
        };
        let entry = FilesystemEntry::from_protocol_entry(&msg).unwrap();
        assert_eq!(entry.entry_type, FilesystemEntryType::SymbolicLink);
        assert_eq!(entry.symlink_target_raw.as_deref(), Some(b"target".as_slice()));
        assert_eq!(entry.symlink_target_display.as_deref(), Some("target"));
    }

    #[test]
    fn hidden_entry_detected() {
        let msg = tuxstack_fs_protocol::HelperMessage::Entry {
            name_b64: tuxstack_fs_protocol::encode_base64(b".hidden"),
            path_token: FilesystemPathToken::from_relative(".hidden").unwrap().0,
            file_type: tuxstack_fs_protocol::HelperFileType::File,
            size: None,
            mtime: None,
            mode: None,
            uid: None,
            gid: None,
            symlink_target_b64: None,
            readable: true,
        };
        let entry = FilesystemEntry::from_protocol_entry(&msg).unwrap();
        assert!(entry.hidden);
    }
}
