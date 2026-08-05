#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;

use tuxstack_docker_core::vfs_providers::{
    ContainerArchiveProvider, ContentSpool, DockerContainerArchiveSource, TarLimits,
};
use tuxstack_docker_core::{
    ContainerTerminalOptions, ContainerTerminalOutput, DockerClient, DockerServices,
};
use tuxstack_domain::{
    CreateContainerMount, CreateContainerRequest, CreateVolumeRequest, RemoveContainerOptions,
    RemoveVolumeOptions,
};
use tuxstack_vfs::{
    FuseNameCodec, ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualPath,
};

struct HybridGuard {
    child: Option<Child>,
    mount: std::path::PathBuf,
    services: DockerServices,
    container_id: Option<String>,
    volume_name: Option<String>,
}

impl HybridGuard {
    fn stop_daemon(&mut self) {
        if let Some(mut child) = self.child.take() {
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            for _ in 0..200 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
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

    async fn cleanup_resources(&mut self) {
        if let Some(id) = self.container_id.take() {
            let _ = self
                .services
                .containers
                .remove_container(
                    &id,
                    &RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    },
                )
                .await;
        }
        if let Some(name) = self.volume_name.take() {
            let _ = self
                .services
                .volumes
                .remove_volume(&name, RemoveVolumeOptions { force: true })
                .await;
        }
    }
}

impl Drop for HybridGuard {
    fn drop(&mut self) {
        self.stop_daemon();
        let services = self.services.clone();
        let container_id = self.container_id.take();
        let volume_name = self.volume_name.take();
        if container_id.is_none() && volume_name.is_none() {
            return;
        }
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(runtime) = runtime {
                runtime.block_on(async move {
                    if let Some(id) = container_id {
                        let _ = services
                            .containers
                            .remove_container(
                                &id,
                                &RemoveContainerOptions {
                                    force: true,
                                    ..Default::default()
                                },
                            )
                            .await;
                    }
                    if let Some(name) = volume_name {
                        let _ = services
                            .volumes
                            .remove_volume(&name, RemoveVolumeOptions { force: true })
                            .await;
                    }
                });
            }
        })
        .join();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires local Docker, busybox:latest, /dev/fuse, and fusermount3"]
async fn container_tree_switches_snapshot_volume_bind_tmpfs_and_image_providers() {
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
    let client = Arc::new(client);
    let services = DockerServices::new(client.clone());
    if services.system.ping().await.is_err() {
        return;
    }

    let test_id = uuid::Uuid::new_v4().to_string();
    let volume_name = format!("tuxstack-hybrid-volume-{test_id}");
    services
        .volumes
        .create_volume(CreateVolumeRequest {
            name: Some(volume_name.clone()),
            ..Default::default()
        })
        .await
        .expect("create hybrid volume");
    let root = tempfile::tempdir().unwrap();
    let bind_root = root.path().join("bind");
    fs::create_dir(&bind_root).unwrap();
    fs::write(bind_root.join("host.txt"), b"bind-before\n").unwrap();

    let created = services
        .containers
        .create_container(&CreateContainerRequest {
            name: Some(format!("tuxstack-hybrid-container-{test_id}")),
            image: "busybox:latest".into(),
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf 'volume-live\\n' >/data/volume.txt; printf 'tmpfs-live\\n' >/runtime/tmp.txt; ln -s /etc/passwd /absolute-passwd; sleep 120".into(),
            ],
            mounts: vec![
                CreateContainerMount::Volume {
                    source: volume_name.clone(),
                    destination: "/data".into(),
                    read_only: false,
                },
                CreateContainerMount::Bind {
                    source: bind_root.to_string_lossy().into_owned(),
                    destination: "/bind".into(),
                    read_only: false,
                    propagation: None,
                },
                CreateContainerMount::Tmpfs {
                    destination: "/runtime".into(),
                    size_bytes: Some(16 * 1024 * 1024),
                    mode: Some(0o755),
                },
            ],
            create_and_start: true,
            ..Default::default()
        })
        .await
        .expect("create hybrid container");
    assert!(created.started, "hybrid fixture did not start");
    let container_id = created.id;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let fixture_detail = services
        .containers
        .inspect_container(&container_id)
        .await
        .expect("inspect hybrid fixture");
    assert_eq!(fixture_detail.summary.state.as_str(), "running");
    assert!(
        fixture_detail
            .mounts
            .iter()
            .any(|mount| mount.mount_type == "tmpfs" && mount.destination == "/runtime")
    );
    let terminal = services
        .container_terminal
        .connect(
            &container_id,
            ContainerTerminalOptions::default(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("connect fixture terminal");
    let mut terminal_output = terminal.take_output().await.expect("take fixture output");
    terminal
        .write_input(b"cat /runtime/tmp.txt; printf '\\nTUXSTACK_TMPFS_DONE\\n'\n\r".to_vec())
        .await
        .expect("probe fixture tmpfs");
    let terminal_bytes = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = terminal_output.next().await {
            match chunk.expect("fixture terminal output") {
                ContainerTerminalOutput::StdOut(chunk)
                | ContainerTerminalOutput::StdErr(chunk)
                | ContainerTerminalOutput::StdIn(chunk)
                | ContainerTerminalOutput::Console(chunk) => bytes.extend(chunk),
            }
            let marker_count = bytes
                .windows(b"TUXSTACK_TMPFS_DONE".len())
                .filter(|window| *window == b"TUXSTACK_TMPFS_DONE")
                .count();
            if marker_count >= 2
                && bytes
                    .windows(b"tmpfs-live".len())
                    .any(|window| window == b"tmpfs-live")
            {
                return bytes;
            }
        }
        bytes
    })
    .await
    .expect("fixture tmpfs terminal probe timed out");
    terminal.close().await;
    assert!(
        terminal_bytes
            .windows(b"tmpfs-live".len())
            .any(|window| window == b"tmpfs-live"),
        "the running container must see the tmpfs fixture before Archive API availability is assessed"
    );
    let direct_archive = ContainerArchiveProvider::new(
        container_id.clone(),
        Arc::new(DockerContainerArchiveSource::new(client)),
        ContentSpool::new(root.path().join("direct-spool"), Default::default())
            .await
            .expect("create direct archive spool"),
        TarLimits::default(),
        Duration::from_secs(30),
    )
    .expect("create direct archive provider");
    let tmpfs_path = VirtualPath::from_absolute(b"/runtime/tmp.txt").unwrap();
    let direct_context = RequestContext {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        pid: std::process::id(),
        request_id: 1,
    };
    let direct_error = direct_archive
        .open(&tmpfs_path, 0, &direct_context)
        .await
        .expect_err("Docker Archive must report the unsupported tmpfs path accurately");
    assert!(matches!(direct_error, VfsError::Unsupported(_)));
    assert_eq!(direct_error.errno(), libc::EOPNOTSUPP);

    let mount = root.path().join("home/TuxStack/docker");
    let mut guard = HybridGuard {
        child: None,
        mount: mount.clone(),
        services: services.clone(),
        container_id: Some(container_id.clone()),
        volume_name: Some(volume_name.clone()),
    };

    let home = root.path().join("home");
    let runtime = root.path().join("runtime");
    let cache = root.path().join("cache");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    guard.child = Some(
        Command::new(env!("CARGO_BIN_EXE_tuxstackd"))
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_CACHE_HOME", &cache)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if Command::new("mountpoint")
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
    .expect("hybrid daemon mount timeout");

    let image_id = services
        .images
        .inspect_image("busybox:latest")
        .await
        .expect("inspect busybox image")
        .summary
        .id;
    let container_root = mount
        .join("containers/.by-id")
        .join(FuseNameCodec::encode(container_id.as_bytes()).unwrap());
    let image_root = mount
        .join("images/.by-id")
        .join(FuseNameCodec::encode(image_id.as_bytes()).unwrap());
    let volume_root = mount
        .join("volumes")
        .join(FuseNameCodec::encode(volume_name.as_bytes()).unwrap());

    assert!(
        fs::read(container_root.join("etc/passwd"))
            .unwrap()
            .starts_with(b"root:")
    );
    assert!(
        fs::read(image_root.join("etc/passwd"))
            .unwrap()
            .starts_with(b"root:")
    );
    assert_eq!(
        fs::read(container_root.join("data/volume.txt")).unwrap(),
        b"volume-live\n"
    );
    assert_eq!(
        fs::read(volume_root.join("volume.txt")).unwrap(),
        b"volume-live\n"
    );
    assert_eq!(
        fs::read(container_root.join("bind/host.txt")).unwrap(),
        b"bind-before\n"
    );
    fs::write(bind_root.join("host.txt"), b"bind-after\n").unwrap();
    assert_eq!(
        fs::read(container_root.join("bind/host.txt")).unwrap(),
        b"bind-after\n"
    );
    let tmpfs_error = fs::read(container_root.join("runtime/tmp.txt"))
        .expect_err("unsupported Docker Archive tmpfs reads must not fall through to rootfs");
    assert_eq!(tmpfs_error.raw_os_error(), Some(libc::EOPNOTSUPP));
    let absolute_link = fs::read_link(container_root.join("absolute-passwd"))
        .expect("read rewritten absolute container symlink");
    assert!(
        !absolute_link.is_absolute(),
        "absolute container links must be rewritten inside the mounted resource"
    );
    assert!(
        Command::new("mountpoint")
            .arg("-q")
            .arg(&mount)
            .status()
            .is_ok_and(|status| status.success()),
        "FUSE mount disconnected before symlink traversal"
    );
    assert!(
        guard.child.as_mut().unwrap().try_wait().unwrap().is_none(),
        "daemon exited before symlink traversal"
    );
    assert!(
        fs::read(container_root.join("absolute-passwd"))
            .expect("follow rewritten absolute container symlink")
            .starts_with(b"root:")
    );

    let write_error = fs::OpenOptions::new()
        .write(true)
        .open(container_root.join("etc/passwd"))
        .unwrap_err();
    assert_eq!(write_error.raw_os_error(), Some(libc::EROFS));
    assert_eq!(fs::metadata(&container_root).unwrap().uid(), unsafe {
        libc::geteuid()
    });

    guard.stop_daemon();
    guard.cleanup_resources().await;
}
