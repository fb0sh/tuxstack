//! Metadata → protocol message conversion (lstat semantics: symlinks are
//! reported as such, never followed).

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tuxstack_fs_protocol::{HelperFileType, HelperMessage};

use crate::error::{HelperError, Result};

/// Best-effort readability estimate from permission bits. The helper runs as
/// uid 0 with all capabilities dropped, so reading requires at least one
/// read bit (DAC_OVERRIDE is gone).
pub fn is_readable(meta: &std::fs::Metadata) -> bool {
    (meta.mode() & 0o444) != 0
}

/// Convert `symlink_metadata` output into a protocol `Entry`/`Stat`-style
/// field set. `path` is the on-disk path (used for the symlink target);
/// `token` is the token the caller wants attached.
pub fn message_fields(
    path: &Path,
    meta: &std::fs::Metadata,
) -> Result<(HelperFileType, Option<u64>, Option<i64>, Option<u32>, Option<u32>, Option<u32>, Option<String>, bool)> {
    let file_type = HelperFileType::from_mode(meta);
    let size = if file_type.is_directory() {
        None
    } else {
        Some(meta.len())
    };
    let symlink_target_b64 = if file_type == HelperFileType::Symlink {
        let target = std::fs::read_link(path).map_err(|error| HelperError::from_io(path, &error))?;
        use std::os::unix::ffi::OsStrExt;
        Some(tuxstack_fs_protocol::encode_base64(
            target.as_os_str().as_bytes(),
        ))
    } else {
        None
    };
    Ok((
        file_type,
        size,
        Some(meta.mtime()),
        Some(meta.mode() & 0o7777),
        Some(meta.uid()),
        Some(meta.gid()),
        symlink_target_b64,
        is_readable(meta),
    ))
}

/// Build an `Entry` message for a directory entry. `parent_relative` is the
/// decoded relative bytes of the parent directory token.
pub fn entry_message(
    path: &Path,
    name_raw: Vec<u8>,
    parent_relative: &[u8],
    meta: &std::fs::Metadata,
) -> Result<HelperMessage> {
    let (file_type, size, mtime, mode, uid, gid, symlink_target_b64, readable) =
        message_fields(path, meta)?;
    let token = crate::path::child_token(parent_relative, &name_raw)?;
    Ok(HelperMessage::Entry {
        name_b64: tuxstack_fs_protocol::encode_base64(&name_raw),
        path_token: token.0,
        file_type,
        size,
        mtime,
        mode,
        uid,
        gid,
        symlink_target_b64,
        readable,
    })
}

/// Build a `Stat` message for a single path (the token echoes the request).
pub fn stat_message(
    path: &Path,
    token: &str,
    meta: &std::fs::Metadata,
) -> Result<HelperMessage> {
    let (file_type, size, mtime, mode, uid, gid, symlink_target_b64, readable) =
        message_fields(path, meta)?;
    Ok(HelperMessage::Stat {
        path_token: token.to_string(),
        file_type,
        size,
        mtime,
        mode,
        uid,
        gid,
        symlink_target_b64,
        readable,
    })
}
