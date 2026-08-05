//! Phase-3 volume/bind provider integration coverage.
//!
//! This target is intentionally ignored because two tests create real Docker
//! resources/helper containers. Register it in docker-core's manifest when the
//! central `vfs_providers` module is integrated.

use std::fs;
use std::sync::Arc;

use tuxstack_docker_core::vfs_providers::helper_bind::HelperBindProvider;
use tuxstack_docker_core::vfs_providers::volume::NamedVolumeProviderPool;
use tuxstack_docker_core::{
    CreateVolumeRequest, DockerClient, DockerServices, FilesystemService, RemoveVolumeOptions,
};
use tuxstack_vfs::{
    ConsistencyMode, ProviderKind, ReadOnlyFilesystemProvider, RequestContext, VirtualFileName,
    VirtualPath,
};

fn context() -> RequestContext {
    RequestContext {
        uid: 1000,
        gid: 1000,
        pid: 1,
        request_id: 1,
    }
}

#[tokio::test]
#[ignore = "requires a reachable local Docker Engine and the static filesystem helper"]
async fn real_named_volume_reuses_provider_and_helper_session() {
    let client = Arc::new(DockerClient::connect_default().expect("Docker must be reachable"));
    let services = DockerServices::new(Arc::clone(&client));
    let filesystem = Arc::new(FilesystemService::new(client));
    let name = format!("tuxstack-test-vfs-volume-{}", uuid::Uuid::new_v4().simple());
    services
        .volumes
        .create_volume(CreateVolumeRequest {
            name: Some(name.clone()),
            ..Default::default()
        })
        .await
        .expect("create test volume");

    let pool = NamedVolumeProviderPool::new("integration-daemon", filesystem);
    let first = pool.provider(name.clone());
    let second = pool.provider(name.clone());
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.descriptor().kind, ProviderKind::NamedVolumeLive);
    assert_eq!(first.descriptor().consistency, ConsistencyMode::Live);
    first
        .read_dir(&VirtualPath::root(), &context())
        .await
        .expect("list empty volume through helper");

    pool.remove(&name).await.expect("stop exact helper session");
    services
        .volumes
        .remove_volume(&name, RemoveVolumeOptions { force: true })
        .await
        .expect("remove test volume");
}

#[tokio::test]
#[ignore = "requires a reachable local Docker Engine and permission to bind a temp directory"]
async fn real_helper_bind_is_live_read_only_and_cleans_up() {
    let root = tempfile::TempDir::new().expect("temporary bind root");
    fs::write(root.path().join("payload"), b"first").expect("write payload");
    let client = Arc::new(DockerClient::connect_default().expect("Docker must be reachable"));
    let provider = HelperBindProvider::new(client, "integration-daemon", root.path())
        .expect("construct helper bind provider");
    let path = VirtualPath::root()
        .join(&VirtualFileName::new(b"payload").unwrap())
        .unwrap();

    let handle = provider
        .open(&path, libc::O_RDONLY, &context())
        .await
        .expect("open helper-backed payload");
    assert_eq!(
        provider
            .read_at(&handle, 0, 16, &context())
            .await
            .expect("read helper-backed payload"),
        b"first"[..]
    );
    provider.close(handle).await.expect("close handle");
    assert!(
        provider
            .open(&path, libc::O_WRONLY, &context())
            .await
            .is_err()
    );
    provider
        .shutdown()
        .await
        .expect("remove exact helper session");
}
