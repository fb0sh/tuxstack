use std::sync::Arc;

use tuxstack_vfs::{
    ContainerPath, ContainerPathRouter, InMemoryProvider, ProviderKey, ReadOnlyFilesystemProvider,
    ResolvedContainerMount, VirtualPath,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn provider() -> Arc<dyn ReadOnlyFilesystemProvider> {
    Arc::new(InMemoryProvider::new())
}

#[test]
fn component_prefix_does_not_match_lookalike_path() {
    let root = provider();
    let mount_provider = provider();
    let router = ContainerPathRouter::new(
        "container",
        ProviderKey("rootfs".into()),
        root,
        vec![ResolvedContainerMount {
            destination: path(b"/app/data"),
            provider_key: ProviderKey("volume".into()),
            provider_root: VirtualPath::root(),
            provider: mount_provider,
        }],
    )
    .unwrap();

    assert_eq!(
        router
            .route(&ContainerPath(path(b"/app/data/file")))
            .unwrap()
            .provider_key,
        ProviderKey("volume".into())
    );
    assert_eq!(
        router
            .route(&ContainerPath(path(b"/app/data2/file")))
            .unwrap()
            .provider_key,
        ProviderKey("rootfs".into())
    );
}

#[test]
fn deepest_nested_mount_wins_and_provider_root_is_translated() {
    let mounts = vec![
        ResolvedContainerMount {
            destination: path(b"/app"),
            provider_key: ProviderKey("outer".into()),
            provider_root: path(b"/export"),
            provider: provider(),
        },
        ResolvedContainerMount {
            destination: path(b"/app/data/cache"),
            provider_key: ProviderKey("inner".into()),
            provider_root: path(b"/cache-root"),
            provider: provider(),
        },
        ResolvedContainerMount {
            destination: path(b"/app/data"),
            provider_key: ProviderKey("middle".into()),
            provider_root: path(b"/volume-root"),
            provider: provider(),
        },
    ];
    let router = ContainerPathRouter::new(
        "container",
        ProviderKey("rootfs".into()),
        provider(),
        mounts,
    )
    .unwrap();

    let inner = router
        .route(&ContainerPath(path(b"/app/data/cache/item")))
        .unwrap();
    assert_eq!(inner.provider_key, ProviderKey("inner".into()));
    assert_eq!(inner.provider_path.as_bytes(), b"/cache-root/item");

    let middle = router
        .route(&ContainerPath(path(b"/app/data/other")))
        .unwrap();
    assert_eq!(middle.provider_key, ProviderKey("middle".into()));
    assert_eq!(middle.provider_path.as_bytes(), b"/volume-root/other");

    let outer = router.route(&ContainerPath(path(b"/app/bin"))).unwrap();
    assert_eq!(outer.provider_key, ProviderKey("outer".into()));
    assert_eq!(outer.provider_path.as_bytes(), b"/export/bin");
}

#[test]
fn mount_at_container_root_is_rejected() {
    assert!(
        ContainerPathRouter::new(
            "container",
            ProviderKey("rootfs".into()),
            provider(),
            vec![ResolvedContainerMount {
                destination: VirtualPath::root(),
                provider_key: ProviderKey("bad".into()),
                provider_root: VirtualPath::root(),
                provider: provider(),
            }],
        )
        .is_err()
    );
}
