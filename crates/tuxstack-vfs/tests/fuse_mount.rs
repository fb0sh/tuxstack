#![cfg(all(feature = "fuse-integration", target_os = "linux"))]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tuxstack_vfs::{
    DockerFilesystemResource, InMemoryProvider, ReadOnlyFilesystemProvider, ReadOnlyFuseAdapter,
    VirtualFileType, VirtualPath, VirtualPathBytes,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn prerequisite(command: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {command} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|status| status.success())
}

/// Real mount test. It is ignored because CI needs a usable `/dev/fuse`, Linux,
/// `fusermount3`, and permission to mount. No Docker provider is involved yet.
#[test]
#[ignore = "requires Linux /dev/fuse and fusermount3"]
fn mount_in_memory_namespace_read_only() {
    assert!(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/fuse")
            .is_ok(),
        "/dev/fuse is unavailable"
    );
    assert!(prerequisite("fusermount3"), "fusermount3 is unavailable");

    let base = std::env::var_os("TMPDIR")
        .map(std::path::PathBuf::from)
        .expect("set a project-local TMPDIR")
        .join(format!("tuxstack-vfs-mount-{}", std::process::id()));
    let mountpoint = base.join("mnt");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&mountpoint).unwrap();

    let provider = Arc::new(InMemoryProvider::new());
    provider
        .add_file(
            path(b"/hello.txt"),
            b"hello-node",
            Bytes::from_static(b"hello from tuxstack\n"),
        )
        .unwrap();
    provider
        .add_symlink(
            path(b"/absolute-link"),
            b"link-node",
            VirtualPathBytes::new(b"/hello.txt").unwrap(),
        )
        .unwrap();
    provider
        .add_special(
            path(b"/character-device"),
            b"device-node",
            VirtualFileType::CharacterDevice,
            0x0103,
        )
        .unwrap();
    let provider: Arc<dyn ReadOnlyFilesystemProvider> = provider;
    let mount_metadata = fs::metadata(&mountpoint).unwrap();
    let adapter = ReadOnlyFuseAdapter::new(
        provider,
        DockerFilesystemResource::Container {
            container_id: "integration".into(),
        },
        "integration-daemon",
        "memory",
        mount_metadata.uid(),
        mount_metadata.gid(),
        Duration::from_secs(2),
        16,
        32,
        Duration::from_secs(60),
    )
    .unwrap();
    let session = adapter.spawn_mount(&mountpoint).unwrap();

    assert_eq!(
        fs::read(mountpoint.join("hello.txt")).unwrap(),
        b"hello from tuxstack\n"
    );
    assert_eq!(
        fs::read_link(mountpoint.join("absolute-link")).unwrap(),
        std::path::PathBuf::from("hello.txt")
    );
    assert_eq!(
        fs::read(mountpoint.join("absolute-link")).unwrap(),
        b"hello from tuxstack\n"
    );
    assert_eq!(
        fs::OpenOptions::new()
            .write(true)
            .open(mountpoint.join("hello.txt"))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EROFS)
    );
    let special_error = fs::OpenOptions::new()
        .read(true)
        .open(mountpoint.join("character-device"))
        .unwrap_err()
        .raw_os_error();
    assert!(
        matches!(special_error, Some(libc::ENXIO | libc::EACCES | libc::EPERM)),
        "special node open must be denied safely, got {special_error:?}"
    );

    session.umount_and_join().unwrap();
    fs::remove_dir_all(base).unwrap();
}
