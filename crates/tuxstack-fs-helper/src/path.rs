//! Root confinement, token ↔ filesystem path mapping, and the `readlink`
//! command.
//!
//! Tokens are opaque, versioned, base64-encoded root-relative paths (see
//! `tuxstack-fs-protocol`). The helper joins them onto the browse root and
//! re-validates them; read operations additionally canonicalize and verify
//! the resolved path stays inside the root so symlinks cannot smuggle reads
//! outside the browsed filesystem.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

use tuxstack_fs_protocol::{
    FilesystemPathToken, HelperErrorCode, HelperMessage,
};

use crate::error::{HelperError, Result};
use crate::emit;

const MAX_SYMLINK_HOPS: usize = 40;

// ---------------------------------------------------------------------------
// Token → path
// ---------------------------------------------------------------------------

/// Decode a token and join it onto `root`. The token itself is validated by
/// the protocol crate (no `..`, no NUL, no absolute paths, no empty
/// components), so the joined path can never lexically escape the root.
pub fn resolve_token(root: &Path, token: &str) -> Result<PathBuf> {
    let parsed = FilesystemPathToken(token.to_string());
    let relative = parsed
        .decode_relative()
        .map_err(|message| HelperError::new(HelperErrorCode::InvalidToken, message))?;
    let mut full = root.as_os_str().as_bytes().to_vec();
    if !relative.is_empty() {
        if !full.ends_with(b"/") {
            full.push(b'/');
        }
        full.extend_from_slice(&relative);
    }
    Ok(PathBuf::from(OsString::from_vec(full)))
}

/// Build the token for `name` inside the directory identified by
/// `parent_relative` (the decoded relative bytes of the parent's token).
pub fn child_token(parent_relative: &[u8], name: &[u8]) -> Result<FilesystemPathToken> {
    let mut relative = Vec::with_capacity(parent_relative.len() + 1 + name.len());
    relative.extend_from_slice(parent_relative);
    if !parent_relative.is_empty() {
        relative.push(b'/');
    }
    relative.extend_from_slice(name);
    FilesystemPathToken::encode_relative(&relative)
        .map_err(|message| HelperError::new(HelperErrorCode::InvalidToken, message))
}

/// Convert an absolute path that is known to live under `root` back into a
/// root-relative token.
pub fn token_under_root(root: &Path, absolute: &Path) -> Result<FilesystemPathToken> {
    let root_bytes = root.as_os_str().as_bytes();
    let path_bytes = absolute.as_os_str().as_bytes();
    let relative = if path_bytes == root_bytes {
        Vec::new()
    } else if let Some(rest) = path_bytes.strip_prefix(root_bytes) {
        let rest = rest.strip_prefix(b"/").unwrap_or(rest);
        if rest.is_empty() {
            Vec::new()
        } else {
            rest.to_vec()
        }
    } else {
        return Err(HelperError::new(
            HelperErrorCode::PathEscapeRejected,
            format!("path escapes browse root: {}", absolute.display()),
        ));
    };
    FilesystemPathToken::encode_relative(&relative)
        .map_err(|message| HelperError::new(HelperErrorCode::InvalidToken, message))
}

// ---------------------------------------------------------------------------
// Canonicalization / confinement
// ---------------------------------------------------------------------------

/// Lexically normalize a path, collapsing `.` and `..` components.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.as_os_str().is_empty() {
                    out = PathBuf::from("/");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else if path.is_absolute() && !out.as_os_str().as_bytes().starts_with(b"/") {
        PathBuf::from(format!("/{}", out.display()))
    } else {
        out
    }
}

/// Canonical absolute path (resolves *every* component's symlinks, like
/// `realpath`). Requires the final target to exist. Handles chained and
/// intermediate symlinks by splicing resolved targets into the path string
/// and re-walking until a fixpoint.
pub fn canonicalize(path: &Path) -> Result<PathBuf> {
    let mut hops = 0usize;
    let mut current = normalize(path);
    loop {
        let mut result = PathBuf::from("/");
        let mut restarted = false;
        let components: Vec<_> = current.components().collect();
        for (index, component) in components.iter().enumerate() {
            let next = result.join(component.as_os_str());
            let meta = std::fs::symlink_metadata(&next)
                .map_err(|error| HelperError::from_io(&next, &error))?;
            if meta.file_type().is_symlink() {
                hops += 1;
                if hops > MAX_SYMLINK_HOPS {
                    return Err(HelperError::new(
                        HelperErrorCode::SymlinkLoop,
                        format!("too many levels of symbolic links at {}", path.display()),
                    ));
                }
                let target = std::fs::read_link(&next)
                    .map_err(|error| HelperError::from_io(&next, &error))?;
                let rest: PathBuf = components[index + 1..]
                    .iter()
                    .map(|c| c.as_os_str())
                    .collect();
                current = if target.is_absolute() {
                    normalize(&target.join(rest))
                } else {
                    normalize(&result.join(target).join(rest))
                };
                restarted = true;
                break;
            }
            result = next;
        }
        if !restarted {
            return Ok(result);
        }
    }
}

/// Resolve `path` and verify the canonical location stays under `root`.
/// Returns the canonical path to use for file reads. Directories and symlink
/// targets that would leave the root are rejected.
pub fn confine_to_root(root: &Path, path: &Path) -> Result<PathBuf> {
    let canonical = canonicalize(path)?;
    let root_canonical = canonicalize(root)?;
    // Component-wise prefix check: `canonical == root` or strictly below it.
    if !(canonical == root_canonical || canonical.starts_with(&root_canonical)) {
        return Err(HelperError::new(
            HelperErrorCode::PathEscapeRejected,
            format!("path escapes browse root: {}", path.display()),
        ));
    }
    Ok(canonical)
}

// ---------------------------------------------------------------------------
// `readlink` command (protocol extension: emits a `Resolved` message)
// ---------------------------------------------------------------------------

pub fn run_readlink(args: &[String]) -> Result<()> {
    let flags = parse_flags(args)?;
    let path = resolve_token(&flags.root, &flags.token)?;
    let canonical = canonicalize(&path)?;
    let token = token_under_root(&flags.root, &canonical)?;
    emit(&HelperMessage::Resolved {
        path_token: token.0,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Flag parsing (shared by list/stat/preview/hash/readlink)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Flags {
    pub root: PathBuf,
    pub token: String,
    pub show_hidden: bool,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub offset: u64,
    pub read_limit: u64,
    pub algorithm: String,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
            token: String::new(),
            show_hidden: false,
            limit: None,
            cursor: None,
            offset: 0,
            read_limit: 64 * 1024,
            algorithm: "sha256".into(),
        }
    }
}

pub fn parse_flags(args: &[String]) -> Result<Flags> {
    let mut flags = Flags::default();
    let mut iter = args.iter();
    let mut seen_token = false;
    while let Some(arg) = iter.next() {
        let value = |iter: &mut std::slice::Iter<'_, String>| {
            iter.next()
                .cloned()
                .ok_or_else(|| HelperError::new(HelperErrorCode::InvalidArgs, format!("missing value for {arg}")))
        };
        match arg.as_str() {
            "--root" => flags.root = PathBuf::from(value(&mut iter)?),
            "--path-token" => {
                flags.token = value(&mut iter)?;
                seen_token = true;
            }
            "--show-hidden" => flags.show_hidden = true,
            "--limit" => {
                let raw = value(&mut iter)?;
                flags.limit = Some(raw.parse().map_err(|_| {
                    HelperError::new(HelperErrorCode::InvalidArgs, format!("invalid --limit: {raw}"))
                })?);
            }
            "--cursor" => flags.cursor = Some(value(&mut iter)?),
            "--offset" => {
                let raw = value(&mut iter)?;
                flags.offset = raw.parse().map_err(|_| {
                    HelperError::new(HelperErrorCode::InvalidArgs, format!("invalid --offset: {raw}"))
                })?;
            }
            "--limit-bytes" => {
                let raw = value(&mut iter)?;
                flags.read_limit = raw.parse().map_err(|_| {
                    HelperError::new(HelperErrorCode::InvalidArgs, format!("invalid --limit-bytes: {raw}"))
                })?;
            }
            "--algorithm" => flags.algorithm = value(&mut iter)?,
            other => {
                return Err(HelperError::new(
                    HelperErrorCode::InvalidArgs,
                    format!("unknown flag: {other}"),
                ))
            }
        }
    }
    if !seen_token {
        return Err(HelperError::new(
            HelperErrorCode::InvalidArgs,
            "missing --path-token",
        ));
    }
    Ok(flags)
}

/// Decode the token argument into validated relative bytes.
pub fn decode_token(token: &str) -> Result<Vec<u8>> {
    FilesystemPathToken(token.to_string())
        .decode_relative()
        .map_err(|message| HelperError::new(HelperErrorCode::InvalidToken, message))
}
