use std::sync::Arc;

use tuxstack_vfs::{
    InMemoryProvider, NamespaceProvider, ReadOnlyFilesystemProvider, RequestContext,
    VirtualFileName, VirtualPath, VirtualPathBytes,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn context() -> RequestContext {
    RequestContext {
        uid: 1000,
        gid: 1000,
        pid: 1,
        request_id: 1,
    }
}

fn provider(file: &[u8], content: &'static [u8]) -> Arc<dyn ReadOnlyFilesystemProvider> {
    let provider = Arc::new(InMemoryProvider::new());
    provider.add_file(path(file), file, content).unwrap();
    provider
}

#[tokio::test]
async fn synthetic_parents_route_to_deepest_component_prefix() {
    let namespace = NamespaceProvider::new();
    namespace
        .mount(
            path(b"/containers/c/root"),
            "root",
            provider(b"/root-file", b"root"),
        )
        .unwrap();
    namespace
        .mount(
            path(b"/containers/c/root/app/data"),
            "volume",
            provider(b"/live-file", b"live"),
        )
        .unwrap();

    let root = namespace
        .read_dir(&VirtualPath::root(), &context())
        .await
        .unwrap();
    assert_eq!(root[0].name.as_bytes(), b"containers");
    let route = namespace
        .provider_at(&path(b"/containers/c/root/app/data/live-file"))
        .unwrap()
        .unwrap();
    assert_eq!(route.key, "volume");
    assert_eq!(route.relative_path.as_bytes(), b"/live-file");
    assert!(
        namespace
            .getattr(&path(b"/containers/c/root/app/data/live-file"), &context())
            .await
            .is_ok()
    );
    assert!(
        namespace
            .getattr(&path(b"/containers/c/root/app/data2/live-file"), &context())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn aliases_are_links_and_handles_stay_with_the_opened_provider() {
    let namespace = NamespaceProvider::new();
    namespace
        .mount(
            path(b"/volumes/.by-id/data"),
            "data",
            provider(b"/value", b"one"),
        )
        .unwrap();
    namespace
        .alias(
            path(b"/volumes/data"),
            VirtualPathBytes::new(b"/volumes/.by-id/data").unwrap(),
        )
        .unwrap();
    assert_eq!(
        namespace
            .read_link(&path(b"/volumes/data"), &context())
            .await
            .unwrap()
            .as_bytes(),
        b"/volumes/.by-id/data"
    );

    let handle = namespace
        .open(
            &path(b"/volumes/.by-id/data/value"),
            libc::O_RDONLY,
            &context(),
        )
        .await
        .unwrap();
    namespace.unmount(&path(b"/volumes/.by-id/data")).unwrap();
    assert_eq!(
        namespace.read_at(&handle, 0, 8, &context()).await.unwrap(),
        "one"
    );
    namespace.close(handle).await.unwrap();
}

#[tokio::test]
async fn provider_absolute_links_are_anchored_inside_the_resource_mount() {
    let provider = Arc::new(InMemoryProvider::new());
    provider
        .add_file(path(b"/inside"), b"inside", b"safe".as_slice())
        .unwrap();
    provider
        .add_symlink(
            path(b"/absolute"),
            b"absolute",
            VirtualPathBytes::new(b"/inside").unwrap(),
        )
        .unwrap();
    let namespace = NamespaceProvider::new();
    namespace
        .mount(path(b"/containers/c"), "container", provider)
        .unwrap();

    assert_eq!(
        namespace
            .read_link(&path(b"/containers/c/absolute"), &context())
            .await
            .unwrap()
            .as_bytes(),
        b"/containers/c/inside"
    );
}

#[test]
fn aliases_require_absolute_non_root_targets() {
    let namespace = NamespaceProvider::new();
    assert!(
        namespace
            .alias(path(b"/alias"), VirtualPathBytes::new(b"relative").unwrap())
            .is_err()
    );
    assert!(VirtualFileName::new(b"data/other").is_err());
}
