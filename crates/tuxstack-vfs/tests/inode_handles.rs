use std::sync::Arc;
use std::time::{Duration, Instant};

use tuxstack_vfs::{
    DockerFilesystemResource, HardlinkKey, InMemoryProvider, InodeTable, OpenHandle,
    OpenHandleTable, ProviderFileHandle, ROOT_INODE, ReadOnlyFilesystemProvider, VfsError,
    VirtualNodeKey, VirtualPath,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn key(value: &[u8], generation: u64) -> VirtualNodeKey {
    VirtualNodeKey {
        daemon_identity: "daemon".into(),
        resource_kind: "container".into(),
        resource_id: "container-id".into(),
        provider_key: "rootfs".into(),
        logical_path: path(value),
        generation,
    }
}

fn hardlink(node_id: &[u8], generation: u64) -> HardlinkKey {
    HardlinkKey {
        daemon_identity: "daemon".into(),
        resource_kind: "container".into(),
        resource_id: "container-id".into(),
        provider_key: "rootfs".into(),
        provider_node_id: node_id.into(),
        generation,
    }
}

#[test]
fn root_is_one_and_allocations_are_stable_and_sequential_not_hashes() {
    let mut table = InodeTable::new();
    assert_eq!(table.lookup(&VirtualNodeKey::root()), Some(ROOT_INODE));
    let first = table.inode_for(key(b"/a", 0), None).unwrap();
    assert_eq!(first, 2);
    assert_eq!(table.inode_for(key(b"/a", 0), None).unwrap(), first);
    let second = table.inode_for(key(b"/b", 0), None).unwrap();
    assert_eq!(second, 3);
    assert_ne!(first, second);
}

#[test]
fn alias_and_hardlink_share_inode_while_generation_does_not() {
    let mut table = InodeTable::new();
    let stable = table
        .inode_for(key(b"/.by-id/id", 1), Some(hardlink(b"node", 1)))
        .unwrap();
    table.add_alias(stable, key(b"/friendly", 1)).unwrap();
    assert_eq!(table.lookup(&key(b"/friendly", 1)), Some(stable));

    let other_path = table
        .inode_for(key(b"/hardlink", 1), Some(hardlink(b"node", 1)))
        .unwrap();
    assert_eq!(other_path, stable);
    let next_generation = table
        .inode_for(key(b"/hardlink", 2), Some(hardlink(b"node", 2)))
        .unwrap();
    assert_ne!(next_generation, stable);
}

#[test]
fn rename_preserves_inode_delete_tombstones_and_numbers_are_never_reused() {
    let mut table = InodeTable::new();
    let old = key(b"/old", 0);
    let inode = table.inode_for(old.clone(), None).unwrap();
    let new = key(b"/new", 0);
    assert_eq!(table.rename_alias(&old, new.clone()).unwrap(), inode);
    assert_eq!(table.lookup(&old), None);
    assert_eq!(table.lookup(&new), Some(inode));

    table.delete(inode).unwrap();
    assert_eq!(table.lookup(&new), None);
    assert!(table.get(inode).unwrap().deleted);
    let replacement = table.inode_for(key(b"/replacement", 0), None).unwrap();
    assert!(replacement > inode);
}

fn open_handle() -> OpenHandle {
    let provider: Arc<dyn ReadOnlyFilesystemProvider> = Arc::new(InMemoryProvider::new());
    OpenHandle {
        provider,
        provider_handle: ProviderFileHandle {
            id: 1,
            path: path(b"/file"),
            content_generation: 2,
        },
        resource: DockerFilesystemResource::Container {
            container_id: "id".into(),
        },
        path: path(b"/file"),
        content_generation: 2,
        backing_strategy: "memory".into(),
        opened_at: Instant::now(),
        last_accessed_at: Instant::now(),
    }
}

#[test]
fn handle_limit_lifecycle_generation_and_idle_expiration() {
    assert!(OpenHandleTable::new(0, Duration::from_secs(1)).is_err());
    let mut table = OpenHandleTable::new(1, Duration::ZERO).unwrap();
    let first = table.insert(open_handle()).unwrap();
    assert_eq!(table.insert(open_handle()), Err(VfsError::TooManyHandles));
    assert_eq!(table.expired_ids(Instant::now()), vec![first]);
    let removed = table.remove(first).unwrap();
    assert_eq!(removed.content_generation, 2);
    assert_eq!(table.remove(first).unwrap_err(), VfsError::BadHandle);

    let second = table.insert(open_handle()).unwrap();
    assert_ne!(
        first, second,
        "slab slot reuse must not revive stale handles"
    );
    assert_eq!(table.len(), 1);
}
