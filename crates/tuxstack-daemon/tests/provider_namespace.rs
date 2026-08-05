#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tuxstack_client::{Client, ClientConfig};
use tuxstack_docker_core::vfs_providers::NamedVolumeProviderPool;
use tuxstack_docker_core::{
    CreateVolumeRequest, DockerClient, DockerConfig, DockerServices, FilesystemService,
    RemoveVolumeOptions,
};
use tuxstack_protocol::{
    ConsistencyMode, DockerResourceRef, ProviderKind, ProviderStatus, Request, Response,
};
use tuxstack_vfs::{
    FuseNameCodec, InMemoryProvider, NamespaceProvider, ReadOnlyFilesystemProvider, RequestContext,
    VirtualPath, VirtualPathBytes,
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

fn memory_file(name: &[u8], contents: &'static [u8]) -> Arc<dyn ReadOnlyFilesystemProvider> {
    let provider = Arc::new(InMemoryProvider::new());
    provider.add_file(path(name), name, contents).unwrap();
    provider
}

#[tokio::test]
async fn encoded_resource_roots_and_friendly_aliases_resolve_through_namespace() {
    let namespace = NamespaceProvider::new();
    let image_id = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let encoded_id = FuseNameCodec::encode(image_id.as_bytes()).unwrap();
    let image_path = path(format!("/images/.by-id/{encoded_id}").as_bytes());
    namespace
        .mount(
            image_path.clone(),
            format!("image:{image_id}"),
            memory_file(b"/image-file", b"image"),
        )
        .unwrap();

    let tag = "registry.example:5000/team/image:latest";
    let encoded_tag = FuseNameCodec::encode(tag.as_bytes()).unwrap();
    let alias_path = path(format!("/images/{encoded_tag}").as_bytes());
    namespace
        .alias(
            alias_path.clone(),
            VirtualPathBytes::new(image_path.as_bytes()).unwrap(),
        )
        .unwrap();

    assert_eq!(
        namespace
            .read_link(&alias_path, &context())
            .await
            .unwrap()
            .as_bytes(),
        image_path.as_bytes()
    );
    let route = namespace
        .provider_at(&path(
            format!("/images/.by-id/{encoded_id}/image-file").as_bytes(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(route.key, format!("image:{image_id}"));
    assert_eq!(route.relative_path.as_bytes(), b"/image-file");
}

#[tokio::test]
async fn deepest_component_mount_wins_without_string_prefix_matching() {
    let namespace = NamespaceProvider::new();
    namespace
        .mount(
            path(b"/containers/.by-id/container/app"),
            "shallow",
            memory_file(b"/shallow", b"one"),
        )
        .unwrap();
    namespace
        .mount(
            path(b"/containers/.by-id/container/app/data"),
            "deep",
            memory_file(b"/deep", b"two"),
        )
        .unwrap();

    let deep = namespace
        .provider_at(&path(b"/containers/.by-id/container/app/data/deep"))
        .unwrap()
        .unwrap();
    assert_eq!(deep.key, "deep");
    assert_eq!(deep.relative_path.as_bytes(), b"/deep");

    let sibling = namespace
        .provider_at(&path(b"/containers/.by-id/container/app/data2"))
        .unwrap()
        .unwrap();
    assert_eq!(sibling.key, "shallow");
    assert_eq!(sibling.relative_path.as_bytes(), b"/data2");
}

#[test]
fn named_volume_pool_returns_the_same_provider_for_top_level_and_container_routes() {
    let client = Arc::new(
        DockerClient::connect_with_config(DockerConfig {
            host: Some("tcp://provider-namespace.invalid:2375".into()),
            ..DockerConfig::default()
        })
        .expect("construct lazy Docker client"),
    );
    let pool = NamedVolumeProviderPool::new("daemon-a", Arc::new(FilesystemService::new(client)));

    let top_level = pool.provider("shared-data");
    let container_mount = pool.provider("shared-data");
    assert!(Arc::ptr_eq(&top_level, &container_mount));
}

struct RealDaemonGuard {
    child: Option<Child>,
    mount: std::path::PathBuf,
}

impl RealDaemonGuard {
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            for _ in 0..100 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if Command::new("mountpoint")
            .arg("-q")
            .arg(&self.mount)
            .status()
            .is_ok_and(|status| status.success())
        {
            let _ = Command::new("fusermount3")
                .args(["-u", "-z"])
                .arg(&self.mount)
                .status();
        }
    }
}

impl Drop for RealDaemonGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires local Docker, /dev/fuse, and fusermount3"]
async fn real_daemon_reports_only_an_attached_volume_provider_as_live() {
    if !std::path::Path::new("/dev/fuse").exists()
        || Command::new("fusermount3")
            .arg("--version")
            .output()
            .is_err()
    {
        return;
    }
    let Ok(client) = DockerClient::connect_default() else {
        return;
    };
    let services = DockerServices::new(Arc::new(client));
    if services.system.ping().await.is_err() {
        return;
    }
    let volume = format!("tuxstack-provider-namespace-{}", uuid::Uuid::new_v4());
    services
        .volumes
        .create_volume(CreateVolumeRequest {
            name: Some(volume.clone()),
            ..Default::default()
        })
        .await
        .expect("create provider namespace test volume");
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let runtime = root.path().join("runtime");
    let cache = root.path().join("cache");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let mount = home.join("TuxStack/docker");
    let child = Command::new(env!("CARGO_BIN_EXE_tuxstackd"))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CACHE_HOME", &cache)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = RealDaemonGuard {
        child: Some(child),
        mount: mount.clone(),
    };
    let socket = runtime.join("tuxstack/control.sock");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if socket.exists()
                && Command::new("mountpoint")
                    .arg("-q")
                    .arg(&mount)
                    .status()
                    .is_ok_and(|status| status.success())
            {
                break;
            }
            assert!(
                guard.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "daemon exited before mounting"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("daemon mount timeout");

    let client = Client::connect(
        ClientConfig::from_runtime_dir(&runtime, "provider-namespace-test").unwrap(),
    )
    .await
    .unwrap();
    let resource = DockerResourceRef::Volume {
        volume_name: volume.clone(),
    };
    let Response::ResourceFusePath(response) = client
        .request(Request::GetResourceFusePath(resource.clone()))
        .await
        .unwrap()
    else {
        panic!("unexpected resource-path response");
    };
    assert_eq!(response.resource, resource);
    assert!(response.path.starts_with(&mount));
    assert_eq!(response.descriptor.kind, ProviderKind::NamedVolumeLive);
    assert_eq!(response.descriptor.consistency, ConsistencyMode::Live);
    assert_eq!(response.descriptor.status, ProviderStatus::Ready);

    guard.stop();
    services
        .volumes
        .remove_volume(&volume, RemoveVolumeOptions { force: true })
        .await
        .expect("remove provider namespace test volume");
}
