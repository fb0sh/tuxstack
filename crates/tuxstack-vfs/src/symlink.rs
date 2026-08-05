use std::collections::HashSet;

use crate::{VfsError, VirtualFileName, VirtualPath, VirtualPathBytes};

pub const DEFAULT_MAX_SYMLINK_DEPTH: usize = 40;

/// Rewrites an absolute provider link to a relative link whose resolution cannot leave
/// the Docker resource root. Relative links are preserved after escape validation.
pub fn rewrite_symlink(
    link_path: &VirtualPath,
    target: &VirtualPathBytes,
) -> Result<VirtualPathBytes, VfsError> {
    let parent = link_path
        .parent()
        .ok_or(VfsError::InvalidInput("resource root cannot be a symlink"))?;
    let _ = resolve_target(&parent, target)?;
    if !target.is_absolute() {
        return Ok(target.clone());
    }

    let normalized_target = VirtualPath::from_absolute(target.as_bytes())?;
    let mut rewritten = Vec::new();
    for _ in 0..parent.depth() {
        if !rewritten.is_empty() {
            rewritten.push(b'/');
        }
        rewritten.extend_from_slice(b"..");
    }
    for component in normalized_target.components() {
        if !rewritten.is_empty() {
            rewritten.push(b'/');
        }
        rewritten.extend_from_slice(component.as_bytes());
    }
    if rewritten.is_empty() {
        rewritten.push(b'.');
    }
    VirtualPathBytes::new(rewritten)
}

/// Resolves a target against a resource-relative parent. Any `..` above resource root
/// is rejected; callers can safely route the resulting path through ContainerPathRouter.
pub fn resolve_target(
    link_parent: &VirtualPath,
    target: &VirtualPathBytes,
) -> Result<VirtualPath, VfsError> {
    let mut components = if target.is_absolute() {
        Vec::new()
    } else {
        link_parent.components().to_vec()
    };
    let bytes = target.as_bytes();
    for raw in bytes
        .split(|byte| *byte == b'/')
        .skip(usize::from(target.is_absolute()))
    {
        if raw.is_empty() || raw == b"." {
            continue;
        }
        if raw == b".." {
            components.pop().ok_or(VfsError::SymlinkEscape)?;
        } else {
            components.push(VirtualFileName::new(raw)?);
        }
    }
    VirtualPath::from_components(components)
}

/// Resolves a chain supplied by `read_link`. `None` means the current path is not a
/// symlink (including a broken terminal target). Every produced absolute path remains
/// resource-relative and can be re-routed across nested container mounts.
pub fn resolve_symlink_chain<F>(
    start: &VirtualPath,
    max_depth: usize,
    mut read_link: F,
) -> Result<VirtualPath, VfsError>
where
    F: FnMut(&VirtualPath) -> Result<Option<VirtualPathBytes>, VfsError>,
{
    if max_depth == 0 {
        return Err(VfsError::SymlinkLoop);
    }
    let mut current = start.clone();
    let mut visited = HashSet::new();
    for _ in 0..max_depth {
        if !visited.insert(current.clone()) {
            return Err(VfsError::SymlinkLoop);
        }
        let Some(target) = read_link(&current)? else {
            return Ok(current);
        };
        let parent = current
            .parent()
            .ok_or(VfsError::InvalidInput("resource root cannot be a symlink"))?;
        current = resolve_target(&parent, &target)?;
    }
    Err(VfsError::SymlinkLoop)
}
