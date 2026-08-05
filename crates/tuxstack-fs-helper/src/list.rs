//! `list` command: single-directory, non-recursive listing with stable
//! byte-order sorting, hidden-file filtering, cursor pagination and
//! runtime-mount exclusion.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tuxstack_fs_protocol::HelperMessage;

use crate::emit;
use crate::error::{HelperError, Result};
use crate::metadata::entry_message;
use crate::path::{self, decode_token, resolve_token};

/// Runtime-injected directories at the container root that are not part of
/// the image (mounted by the Docker runtime): always hidden.
const RUNTIME_ROOT_DIRS: &[&[u8]] = &[b"proc", b"sys", b"dev", b"run"];
/// The helper's own installation directory (inside the container, at the
/// image root): always hidden.
const HELPER_ROOT_NAME: &[u8] = b".tuxstack";

pub fn run(args: &[String]) -> Result<()> {
    let flags = path::parse_flags(args)?;
    let dir = resolve_token(&flags.root, &flags.token)?;
    let relative = decode_token(&flags.token)?;
    let is_browse_root = relative.is_empty();

    // Distinguish missing / not-a-directory / permission failures before
    // listing (follows symlinks so a symlink-to-directory still lists).
    let meta = std::fs::metadata(&dir).map_err(|error| HelperError::from_io(&dir, &error))?;
    if !meta.is_dir() {
        return Err(HelperError::new(
            crate::HelperErrorCode::NotDirectory,
            format!("not a directory: {}", dir.display()),
        ));
    }

    let reader = std::fs::read_dir(&dir).map_err(|error| HelperError::from_io(&dir, &error))?;
    let cursor_raw: Option<Vec<u8>> = match flags.cursor.as_deref() {
        Some(cursor) => Some(
            tuxstack_fs_protocol::decode_base64(cursor).map_err(|message| {
                HelperError::new(
                    crate::HelperErrorCode::InvalidToken,
                    format!("invalid cursor: {message}"),
                )
            })?,
        ),
        None => None,
    };
    let mut names: Vec<Vec<u8>> = Vec::new();
    for item in reader {
        let item = item.map_err(|error| HelperError::from_io(&dir, &error))?;
        let raw = item.file_name().as_bytes().to_vec();
        if raw == b"." || raw == b".." {
            continue;
        }
        if is_browse_root
            && (RUNTIME_ROOT_DIRS.contains(&raw.as_slice()) || raw == HELPER_ROOT_NAME)
        {
            continue;
        }
        if !flags.show_hidden && raw.first() == Some(&b'.') {
            continue;
        }
        if cursor_raw
            .as_ref()
            .is_some_and(|c| raw.as_slice() <= c.as_slice())
        {
            continue;
        }
        names.push(raw);
    }
    names.sort_unstable();

    let limit = flags.limit.unwrap_or(usize::MAX);
    let page_limit = limit.min(names.len());
    let truncated = names.len() > page_limit;
    let next_cursor = if truncated {
        Some(tuxstack_fs_protocol::encode_base64(&names[page_limit - 1]))
    } else {
        None
    };
    let page = &names[..page_limit];

    for name in page {
        let full = dir.join(OsStr::from_bytes(name));
        let entry_meta = std::fs::symlink_metadata(&full)
            .map_err(|error| HelperError::from_io(&full, &error))?;
        // One entry may legitimately fail (dangling or permission); per the
        // protocol spec, fail the whole listing instead of silently skipping.
        let message = entry_message(&full, name.clone(), &relative, &entry_meta)?;
        emit(&message);
    }
    emit(&HelperMessage::End {
        truncated,
        next_cursor,
    });
    Ok(())
}
