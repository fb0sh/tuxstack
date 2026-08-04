//! TuxStack filesystem-helper wire protocol.
//!
//! The static `tuxstack-fs-helper` binary runs inside preview containers and
//! speaks JSON Lines on stdout. This crate defines every message, the stable
//! error codes, the path token format and the byte-field encoding so the
//! helper and the daemon-side service share one source of truth.
//!
//! Rules:
//! - One JSON object per line; raw filenames and path bytes are always
//!   base64-encoded (never raw, never assumed UTF-8).
//! - The first message of a session is `Hello`; the daemon must verify
//!   `protocol` before doing anything else.
//! - `list` ends with `End`; `preview` ends with a chunk whose `eof` is true.
//! - Errors are `Error { code, message }` with a stable machine-readable
//!   `code`; the message is only for humans.

use serde::{Deserialize, Serialize};

/// Current wire protocol version. Bump on incompatible changes; the daemon
/// rejects any other value during the `hello` handshake.
pub const FS_HELPER_PROTOCOL_VERSION: u32 = 1;

/// Path-token version prefix (see [`FilesystemPathToken`]).
pub const PATH_TOKEN_VERSION: &str = "v1";

// ---------------------------------------------------------------------------
// File types
// ---------------------------------------------------------------------------

/// Kind of a filesystem entry, serialized snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperFileType {
    Directory,
    File,
    Symlink,
    Socket,
    Fifo,
    BlockDevice,
    CharacterDevice,
    Unknown,
}

impl HelperFileType {
    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }

    /// Unix `st_mode & S_IFMT` classification used by the helper.
    pub fn from_mode(meta: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::FileTypeExt;
        let ft = meta.file_type();
        if ft.is_dir() {
            Self::Directory
        } else if ft.is_file() {
            Self::File
        } else if ft.is_symlink() {
            Self::Symlink
        } else if ft.is_socket() {
            Self::Socket
        } else if ft.is_fifo() {
            Self::Fifo
        } else if ft.is_block_device() {
            Self::BlockDevice
        } else if ft.is_char_device() {
            Self::CharacterDevice
        } else {
            Self::Unknown
        }
    }
}

// ---------------------------------------------------------------------------
// Stable error codes
// ---------------------------------------------------------------------------

/// Stable, machine-readable helper error codes. The daemon maps these onto
/// its own [`FilesystemError`]-style surface; the UI must never match on
/// English message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperErrorCode {
    /// Invalid or undecodable arguments.
    InvalidArgs,
    /// Path token is malformed or unreadable.
    InvalidToken,
    /// Token decoded to a path that escapes the browse root.
    PathEscapeRejected,
    /// The path does not exist.
    NotFound,
    /// The path exists but is not a directory / file as required.
    NotDirectory,
    IsDirectory,
    /// Access denied (EACCES/EPERM).
    PermissionDenied,
    /// Symlink chain exceeds the hop limit.
    SymlinkLoop,
    /// Refused to touch special files (FIFO/socket/device) on preview.
    UnsupportedFileType,
    /// Generic I/O failure.
    Io,
}

impl HelperErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgs => "invalid_args",
            Self::InvalidToken => "invalid_token",
            Self::PathEscapeRejected => "path_escape_rejected",
            Self::NotFound => "not_found",
            Self::NotDirectory => "not_directory",
            Self::IsDirectory => "is_directory",
            Self::PermissionDenied => "permission_denied",
            Self::SymlinkLoop => "symlink_loop",
            Self::UnsupportedFileType => "unsupported_file_type",
            Self::Io => "io",
        }
    }
}

// ---------------------------------------------------------------------------
// Path tokens
// ---------------------------------------------------------------------------

/// Opaque, versioned path token.
///
/// Encoding: `v1:<base64(root-relative raw path bytes)>`. The empty relative
/// path denotes the browse root itself. Tokens are created by the helper
/// (from the root it was started with) and returned to the UI, which must
/// hand them back verbatim for later operations — the UI never splices
/// filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FilesystemPathToken(pub String);

impl FilesystemPathToken {
    /// Encode a root-relative raw path. `relative` must be empty (root) or a
    /// relative path without `..`, NUL bytes or a leading slash.
    pub fn encode_relative(relative: &[u8]) -> Result<Self, String> {
        validate_relative(relative)?;
        Ok(Self(format!(
            "{PATH_TOKEN_VERSION}:{}",
            encode_base64(relative)
        )))
    }

    /// Build a token from a display-style relative path such as `""`, `"etc"`
    /// or `"etc/passwd"`.
    pub fn from_relative(relative: &str) -> Result<Self, String> {
        Self::encode_relative(relative.as_bytes())
    }

    /// Convenience: return the root token (empty relative path).
    pub fn root_token() -> Self {
        Self::from_relative("").expect("root token is always valid")
    }

    /// Decode back to the root-relative raw bytes after validating the
    /// version prefix and rejecting traversal.
    pub fn decode_relative(&self) -> Result<Vec<u8>, String> {
        let Some(rest) = self.0.strip_prefix(&format!("{PATH_TOKEN_VERSION}:")) else {
            return Err(format!("unsupported path token version: {}", self.0));
        };
        let bytes = decode_base64(rest)
            .map_err(|message| format!("invalid path token encoding: {message}"))?;
        validate_relative(&bytes)?;
        Ok(bytes)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.decode_relative().map(|b| b.is_empty()).unwrap_or(false)
    }
}

impl std::fmt::Display for FilesystemPathToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

fn validate_relative(relative: &[u8]) -> Result<(), String> {
    if relative.is_empty() {
        return Ok(());
    }
    if relative[0] == b'/' {
        return Err("path token must be root-relative, not absolute".into());
    }
    if relative.contains(&0) {
        return Err("path token contains a NUL byte".into());
    }
    for part in relative.split(|byte| *byte == b'/') {
        if part == b".." {
            return Err("path token must not contain '..'".into());
        }
        if part == b"." {
            return Err("path token must not contain '.'".into());
        }
        if part.is_empty() {
            return Err("path token must not contain empty components".into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Base64 byte fields
// ---------------------------------------------------------------------------

const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a' + 26) as u32),
            b'0'..=b'9' => Some((byte - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if clean.len() % 4 != 0 {
        return Err("invalid base64 length".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut quad = [0u32; 4];
        for (i, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                quad[i] = 0;
            } else {
                quad[i] = value(*byte)
                    .ok_or_else(|| "invalid base64 character".to_string())?;
            }
        }
        let triple = (quad[0] << 18) | (quad[1] << 12) | (quad[2] << 6) | quad[3];
        out.push((triple >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(triple as u8);
        }
    }
    Ok(out)
}

/// A filename with lossy display text. UI shows `display` only; operations
/// must use `raw` (or the entry's `path_token`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileName {
    pub raw: Vec<u8>,
    pub display: String,
}

impl FileName {
    pub fn from_raw(raw: Vec<u8>) -> Self {
        let display = String::from_utf8_lossy(&raw).into_owned();
        Self { raw, display }
    }
}

// ---------------------------------------------------------------------------
// Wire messages
// ---------------------------------------------------------------------------

/// One JSON Lines message from the helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelperMessage {
    /// First line of every session; the daemon validates `protocol`.
    Hello {
        protocol: u32,
        helper_version: String,
    },
    /// One directory entry (from `list`).
    Entry {
        name_b64: String,
        path_token: String,
        file_type: HelperFileType,
        size: Option<u64>,
        mtime: Option<i64>,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        symlink_target_b64: Option<String>,
        readable: bool,
    },
    /// Result of `stat` for a single path.
    Stat {
        path_token: String,
        file_type: HelperFileType,
        size: Option<u64>,
        mtime: Option<i64>,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        symlink_target_b64: Option<String>,
        readable: bool,
    },
    /// One bounded chunk of file content (from `preview`).
    PreviewChunk {
        data_b64: String,
        offset: u64,
        eof: bool,
        truncated: bool,
    },
    /// Result of `hash`.
    Hash { algorithm: String, value: String },
    /// Result of `readlink` (protocol extension): the canonical root-relative
    /// path token of the resolved target.
    Resolved { path_token: String },
    /// Terminal line of `list`.
    End {
        truncated: bool,
        next_cursor: Option<String>,
    },
    /// Terminal error; always the last line before exit code 1.
    Error {
        code: HelperErrorCode,
        message: String,
    },
}

/// `list` request (helper CLI flags mirror these fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRequest {
    pub path_token: FilesystemPathToken,
    pub show_hidden: bool,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// `stat` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatRequest {
    pub path_token: FilesystemPathToken,
}

/// `preview` request (bounded byte range).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRequest {
    pub path_token: FilesystemPathToken,
    pub offset: u64,
    pub limit: u64,
}

/// `hash` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashRequest {
    pub path_token: FilesystemPathToken,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> HelperMessage {
        HelperMessage::Entry {
            name_b64: encode_base64(b"app.log"),
            path_token: FilesystemPathToken::from_relative("var/log").unwrap().0,
            file_type: HelperFileType::File,
            size: Some(2048),
            mtime: Some(1_722_000_001),
            mode: Some(0o644),
            uid: Some(1000),
            gid: Some(1000),
            symlink_target_b64: None,
            readable: true,
        }
    }

    #[test]
    fn all_messages_json_roundtrip() {
        let messages = vec![
            HelperMessage::Hello {
                protocol: 1,
                helper_version: "0.1.0".into(),
            },
            sample_entry(),
            HelperMessage::Stat {
                path_token: FilesystemPathToken::from_relative("etc").unwrap().0,
                file_type: HelperFileType::Directory,
                size: None,
                mtime: Some(1_722_000_000),
                mode: Some(0o755),
                uid: Some(0),
                gid: Some(0),
                symlink_target_b64: None,
                readable: true,
            },
            HelperMessage::PreviewChunk {
                data_b64: encode_base64(b"hello"),
                offset: 0,
                eof: true,
                truncated: false,
            },
            HelperMessage::Hash {
                algorithm: "sha256".into(),
                value: "abc".into(),
            },
            HelperMessage::Resolved {
                path_token: FilesystemPathToken::from_relative("usr/bin").unwrap().0,
            },
            HelperMessage::End {
                truncated: false,
                next_cursor: None,
            },
            HelperMessage::End {
                truncated: true,
                next_cursor: Some(encode_base64(b"z")),
            },
            HelperMessage::Error {
                code: HelperErrorCode::PermissionDenied,
                message: "permission denied: /x".into(),
            },
        ];
        for message in &messages {
            let json = serde_json::to_string(message).unwrap();
            let parsed: HelperMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, message, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let json = r#"{"kind":"end","truncated":true,"next_cursor":null,"future":"x"}"#;
        let parsed: HelperMessage = serde_json::from_str(json).unwrap();
        assert_eq!(
            parsed,
            HelperMessage::End {
                truncated: true,
                next_cursor: None
            }
        );
    }

    #[test]
    fn error_codes_are_stable() {
        let mut codes = Vec::new();
        for (code, expected) in [
            (HelperErrorCode::InvalidArgs, "invalid_args"),
            (HelperErrorCode::InvalidToken, "invalid_token"),
            (HelperErrorCode::PathEscapeRejected, "path_escape_rejected"),
            (HelperErrorCode::NotFound, "not_found"),
            (HelperErrorCode::NotDirectory, "not_directory"),
            (HelperErrorCode::IsDirectory, "is_directory"),
            (HelperErrorCode::PermissionDenied, "permission_denied"),
            (HelperErrorCode::SymlinkLoop, "symlink_loop"),
            (HelperErrorCode::UnsupportedFileType, "unsupported_file_type"),
            (HelperErrorCode::Io, "io"),
        ] {
            assert_eq!(code.as_str(), expected);
            assert_eq!(
                serde_json::from_str::<HelperErrorCode>(&format!("\"{expected}\"")).unwrap(),
                code
            );
            codes.push(code);
        }
        assert_eq!(codes.len(), 10);
    }

    #[test]
    fn path_token_roundtrip_and_rejects_escape() {
        for relative in ["", "etc", "etc/passwd", "a b/c\nd", "日本"] {
            let token = FilesystemPathToken::from_relative(relative).unwrap();
            assert_eq!(token.decode_relative().unwrap(), relative.as_bytes());
        }
        assert!(FilesystemPathToken::from_relative("../etc").is_err());
        assert!(FilesystemPathToken::from_relative("/etc").is_err());
        assert!(FilesystemPathToken::from_relative("a\0b").is_err());
        assert!(FilesystemPathToken::from_relative("a//b").is_err());
        assert!(FilesystemPathToken::from_relative("a/../b").is_err());
        assert!(FilesystemPathToken::from_relative(".").is_err());
        assert!(FilesystemPathToken("v2:abc".into()).decode_relative().is_err());
        assert!(FilesystemPathToken::from_relative("").unwrap().is_root());
    }

    #[test]
    fn non_utf8_bytes_survive_base64_fields() {
        let raw = b"bad-\xff-name".to_vec();
        let encoded = encode_base64(&raw);
        assert_eq!(decode_base64(&encoded).unwrap(), raw);
        let name = FileName::from_raw(raw);
        assert!(name.display.contains('\u{fffd}'));
    }

    #[test]
    fn base64_roundtrip() {
        for data in [
            b"".as_slice(),
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"\xff\x00\x01abc\xfe",
        ] {
            assert_eq!(decode_base64(&encode_base64(data)).unwrap(), data);
        }
    }

    #[test]
    fn request_types_roundtrip() {
        let list = ListRequest {
            path_token: FilesystemPathToken::from_relative("usr/bin").unwrap(),
            show_hidden: true,
            limit: Some(1000),
            cursor: Some(encode_base64(b"zz")),
        };
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(serde_json::from_str::<ListRequest>(&json).unwrap(), list);
    }
}
