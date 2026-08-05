#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use tuxstack_client::{Client, ClientConfig};
use tuxstack_protocol::{DockerResourceRef, MountState, ProtocolErrorCode, Request, Response};

struct DaemonGuard {
    child: Option<Child>,
    mount: std::path::PathBuf,
}

impl DaemonGuard {
    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("daemon child is present")
    }

    fn stop(&mut self) -> Option<ExitStatus> {
        let mut child = self.child.take()?;
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        for _ in 0..200 {
            if let Some(status) = child.try_wait().ok().flatten() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        child.wait().ok()
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.stop();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires local Docker, /dev/fuse, and fusermount3"]
async fn daemon_owns_secure_ipc_and_persistent_readonly_mount() {
    if !std::path::Path::new("/dev/fuse").exists()
        || Command::new("fusermount3")
            .arg("--version")
            .output()
            .is_err()
    {
        eprintln!("skipping: FUSE prerequisites unavailable");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let runtime = root.path().join("runtime");
    let cache = root.path().join("cache");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_tuxstackd"))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CACHE_HOME", &cache)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let socket = runtime.join("tuxstack/control.sock");
    let mount = home.join("TuxStack/docker");
    let mut daemon = DaemonGuard {
        child: Some(child),
        mount: mount.clone(),
    };
    let ready = tokio::time::timeout(Duration::from_secs(20), async {
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
                daemon.child_mut().try_wait().unwrap().is_none(),
                "daemon exited early"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(ready.is_ok(), "daemon did not become ready");

    let metadata = fs::symlink_metadata(&socket).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    let config = ClientConfig::from_runtime_dir(&runtime, "integration-test").unwrap();
    let client = Client::connect(config).await.unwrap();
    client.ping().await.unwrap();
    let Response::DaemonStatus(status) = client.request(Request::GetDaemonStatus).await.unwrap()
    else {
        panic!("unexpected daemon-status response");
    };
    assert!(matches!(status.mount.state, MountState::Mounted));
    assert!(status.mount.read_only);

    let resource = DockerResourceRef::Volume {
        volume_name: "nonexistent-integration-volume".into(),
    };
    let Response::Error(error) = client
        .request(Request::GetResourceFusePath(resource))
        .await
        .unwrap()
    else {
        panic!("unknown resources must not receive guessed FUSE paths");
    };
    assert_eq!(error.code, ProtocolErrorCode::NotFound);

    let status = daemon.stop().expect("stop daemon");
    assert!(status.success());
    assert!(!socket.exists());
    assert!(
        !Command::new("mountpoint")
            .arg("-q")
            .arg(&mount)
            .status()
            .is_ok_and(|status| status.success())
    );
}
