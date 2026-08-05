use std::collections::HashMap;

use tuxstack_vfs::{
    VfsError, VirtualPath, VirtualPathBytes, resolve_symlink_chain, resolve_target, rewrite_symlink,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn target(value: &[u8]) -> VirtualPathBytes {
    VirtualPathBytes::new(value).unwrap()
}

#[test]
fn absolute_container_and_image_targets_are_rewritten_under_resource_root() {
    let rewritten = rewrite_symlink(&path(b"/usr/bin/tool"), &target(b"/lib/tool")).unwrap();
    assert_eq!(rewritten.as_bytes(), b"../../lib/tool");
    assert_eq!(
        resolve_target(&path(b"/usr/bin"), &rewritten).unwrap(),
        path(b"/lib/tool")
    );

    let image = rewrite_symlink(&path(b"/lib64/loader"), &target(b"/usr/lib/loader")).unwrap();
    assert_eq!(image.as_bytes(), b"../usr/lib/loader");
}

#[test]
fn relative_links_are_preserved_but_host_escape_is_rejected() {
    let relative = target(b"../lib/tool");
    assert_eq!(
        rewrite_symlink(&path(b"/usr/tool"), &relative).unwrap(),
        relative
    );
    assert_eq!(
        rewrite_symlink(&path(b"/tool"), &target(b"../../etc/passwd")),
        Err(VfsError::SymlinkEscape)
    );
    assert_eq!(
        resolve_target(&VirtualPath::root(), &target(b"../host")),
        Err(VfsError::SymlinkEscape)
    );
}

#[test]
fn cross_provider_target_normalizes_to_container_path_for_rerouting() {
    let result = resolve_target(&path(b"/app/bin"), &target(b"../data/volume-file")).unwrap();
    assert_eq!(result, path(b"/app/data/volume-file"));
}

#[test]
fn chain_resolution_detects_loops_and_depth_limit() {
    let links = HashMap::from([(path(b"/a"), target(b"/b")), (path(b"/b"), target(b"/a"))]);
    assert_eq!(
        resolve_symlink_chain(&path(b"/a"), 40, |candidate| Ok(links
            .get(candidate)
            .cloned())),
        Err(VfsError::SymlinkLoop)
    );

    let deep = HashMap::from([
        (path(b"/a"), target(b"/b")),
        (path(b"/b"), target(b"/c")),
        (path(b"/c"), target(b"/terminal")),
    ]);
    assert_eq!(
        resolve_symlink_chain(&path(b"/a"), 2, |candidate| Ok(deep
            .get(candidate)
            .cloned())),
        Err(VfsError::SymlinkLoop)
    );
}

#[test]
fn broken_link_resolves_to_safe_terminal_path() {
    let links = HashMap::from([(path(b"/link"), target(b"missing"))]);
    assert_eq!(
        resolve_symlink_chain(&path(b"/link"), 40, |candidate| Ok(links
            .get(candidate)
            .cloned()))
        .unwrap(),
        path(b"/missing")
    );
}
