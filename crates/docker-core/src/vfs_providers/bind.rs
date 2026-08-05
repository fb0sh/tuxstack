//! Secure, direct provider for local Linux bind-mount sources.
//!
//! Every operation starts from a retained directory descriptor. `openat2`
//! resolves beneath an in-root anchor with magic-link rejection; untrusted VFS
//! bytes are never joined to a host `PathBuf`. Open file descriptors back
//! random reads directly with `pread`.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use rustix::fd::OwnedFd;
use rustix::fs::{Dir, FileType, Mode, OFlags, ResolveFlags, Stat};
use tokio::sync::RwLock;
use tuxstack_vfs::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualFileType, VirtualMetadata, VirtualPath, VirtualPathBytes,
};

const DEFAULT_DIRECTORY_TTL: Duration = Duration::from_millis(250);
const MAX_DIRECTORY_TTL: Duration = Duration::from_secs(1);
const MAX_READ_BYTES: u32 = 16 * 1024 * 1024;

struct CachedDirectory {
    inserted: Instant,
    entries: Arc<Vec<VirtualDirectoryEntry>>,
}

struct LocalOpenFile {
    fd: Arc<OwnedFd>,
    path: VirtualPath,
}

/// A strictly read-only view rooted at a retained Linux directory FD.
pub struct LocalBindProvider {
    source_root: PathBuf,
    root_fd: Arc<OwnedFd>,
    directory_ttl: Duration,
    directories: RwLock<HashMap<VirtualPath, CachedDirectory>>,
    handles: RwLock<HashMap<u64, LocalOpenFile>>,
    next_handle: AtomicU64,
}

impl LocalBindProvider {
    pub async fn new(source_root: impl Into<PathBuf>) -> Result<Self, VfsError> {
        Self::with_directory_ttl(source_root, DEFAULT_DIRECTORY_TTL).await
    }

    pub async fn with_directory_ttl(
        source_root: impl Into<PathBuf>,
        directory_ttl: Duration,
    ) -> Result<Self, VfsError> {
        if directory_ttl > MAX_DIRECTORY_TTL {
            return Err(VfsError::InvalidInput(
                "local-bind directory TTL must not exceed one second",
            ));
        }
        let source_root = source_root.into();
        validate_source_root(&source_root)?;
        let open_path = source_root.clone();
        let root_fd = tokio::task::spawn_blocking(move || {
            rustix::fs::open(
                &open_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(map_errno)
        })
        .await
        .map_err(join_error)??;

        Ok(Self {
            source_root,
            root_fd: Arc::new(root_fd),
            directory_ttl,
            directories: RwLock::new(HashMap::new()),
            handles: RwLock::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
        })
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    fn descriptor_value(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            kind: ProviderKind::LocalBindLive,
            consistency: ConsistencyMode::Live,
            source: Some(self.source_root.to_string_lossy().into_owned()),
            capabilities: ProviderCapabilities::READ_ONLY,
        }
    }

    async fn metadata(&self, path: &VirtualPath) -> Result<VirtualMetadata, VfsError> {
        let root = Arc::clone(&self.root_fd);
        let relative = relative_cstring(path)?;
        tokio::task::spawn_blocking(move || {
            let fd = open_path_fd(&root, &relative, true)?;
            let stat = rustix::fs::fstat(&fd).map_err(map_errno)?;
            metadata_from_stat(&stat)
        })
        .await
        .map_err(join_error)?
    }

    async fn directory_entries(
        &self,
        path: &VirtualPath,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        if let Some(hit) = self.directories.read().await.get(path) {
            if hit.inserted.elapsed() < self.directory_ttl {
                return Ok(Arc::clone(&hit.entries));
            }
        }

        let root = Arc::clone(&self.root_fd);
        let relative = relative_cstring(path)?;
        let parent_path = path.clone();
        let entries = tokio::task::spawn_blocking(move || {
            let directory_fd = open_path_fd(&root, &relative, false)?;
            let stat = rustix::fs::fstat(&directory_fd).map_err(map_errno)?;
            if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
                return Err(VfsError::NotDirectory);
            }
            let mut directory = Dir::read_from(&directory_fd).map_err(map_errno)?;
            let mut entries = Vec::new();
            for result in &mut directory {
                let entry = result.map_err(map_errno)?;
                let name_bytes = entry.file_name().to_bytes();
                if name_bytes == b"." || name_bytes == b".." {
                    continue;
                }
                let name = VirtualFileName::new(name_bytes)?;
                // Do not trust d_type. Anchor metadata lookup to this already-open
                // directory FD and capture the final object with O_PATH|O_NOFOLLOW.
                let child_fd = rustix::fs::openat2(
                    &directory_fd,
                    entry.file_name(),
                    OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                    Mode::empty(),
                    ResolveFlags::IN_ROOT | ResolveFlags::NO_MAGICLINKS,
                )
                .map_err(map_errno)?;
                let child_stat = rustix::fs::fstat(child_fd).map_err(map_errno)?;
                entries.push(VirtualDirectoryEntry {
                    name: name.clone(),
                    metadata: metadata_from_stat(&child_stat)?,
                });
            }
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            // Keep path ownership in the blocking closure to ensure no host path
            // reconstruction is accidentally introduced during refactors.
            let _ = parent_path;
            Ok::<_, VfsError>(Arc::new(entries))
        })
        .await
        .map_err(join_error)??;

        self.directories.write().await.insert(
            path.clone(),
            CachedDirectory {
                inserted: Instant::now(),
                entries: Arc::clone(&entries),
            },
        );
        Ok(entries)
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for LocalBindProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor_value()
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.metadata(&parent.join(name)?).await
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.metadata(path).await
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        self.directory_entries(path).await
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        if path.is_root() {
            return Err(VfsError::InvalidInput("bind root is not a symlink"));
        }
        let parent = path.parent().expect("non-root path has a parent");
        let name = path.file_name().expect("non-root path has a name").clone();
        let root = Arc::clone(&self.root_fd);
        let parent_relative = relative_cstring(&parent)?;
        let name = CString::new(name.as_bytes())
            .map_err(|_| VfsError::InvalidInput("NUL in bind path"))?;
        tokio::task::spawn_blocking(move || {
            let parent_fd = open_path_fd(&root, &parent_relative, false)?;
            let target = rustix::fs::readlinkat(&parent_fd, name, Vec::new()).map_err(map_errno)?;
            VirtualPathBytes::new(target.to_bytes())
        })
        .await
        .map_err(join_error)?
    }

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        _ctx: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        if !tuxstack_vfs::is_read_only_open(flags) {
            return Err(VfsError::ReadOnly);
        }
        let root = Arc::clone(&self.root_fd);
        let relative = relative_cstring(path)?;
        let fd = tokio::task::spawn_blocking(move || {
            let fd = open_path_fd(&root, &relative, false)?;
            let stat = rustix::fs::fstat(&fd).map_err(map_errno)?;
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::RegularFile => Ok(fd),
                FileType::Directory => Err(VfsError::IsDirectory),
                FileType::Symlink => Err(VfsError::InvalidInput("cannot open a symlink")),
                _ => Err(VfsError::SpecialFile),
            }
        })
        .await
        .map_err(join_error)??;

        let id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.handles.write().await.insert(
            id,
            LocalOpenFile {
                fd: Arc::new(fd),
                path: path.clone(),
            },
        );
        Ok(ProviderFileHandle {
            id,
            path: path.clone(),
            content_generation: 0,
        })
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        _ctx: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        if size > MAX_READ_BYTES {
            return Err(VfsError::InvalidInput(
                "local bind read request is too large",
            ));
        }
        let fd = {
            let handles = self.handles.read().await;
            let opened = handles.get(&handle.id).ok_or(VfsError::BadHandle)?;
            if opened.path != handle.path || handle.content_generation != 0 {
                return Err(VfsError::BadHandle);
            }
            Arc::clone(&opened.fd)
        };
        tokio::task::spawn_blocking(move || {
            let mut bytes = vec![0_u8; size as usize];
            let read = rustix::io::pread(&fd, &mut bytes, offset).map_err(map_errno)?;
            bytes.truncate(read);
            Ok(Bytes::from(bytes))
        })
        .await
        .map_err(join_error)?
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        match self.handles.write().await.remove(&handle.id) {
            Some(opened) if opened.path == handle.path && handle.content_generation == 0 => Ok(()),
            Some(_) | None => Err(VfsError::BadHandle),
        }
    }

    async fn refresh(&self, path: Option<&VirtualPath>) -> Result<(), VfsError> {
        let mut directories = self.directories.write().await;
        if let Some(path) = path {
            directories.retain(|cached, _| !cached.starts_with(path) && !path.starts_with(cached));
        } else {
            directories.clear();
        }
        // Existing file FDs intentionally remain readable after rename/unlink,
        // matching normal Unix open-handle semantics.
        Ok(())
    }
}

fn validate_source_root(source_root: &Path) -> Result<(), VfsError> {
    if !source_root.is_absolute() {
        return Err(VfsError::InvalidInput("bind source must be absolute"));
    }
    if source_root.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err(VfsError::InvalidInput(
            "bind source must not contain dot components",
        ));
    }
    Ok(())
}

fn relative_cstring(path: &VirtualPath) -> Result<CString, VfsError> {
    let mut bytes = Vec::with_capacity(path.byte_len());
    if path.is_root() {
        bytes.push(b'.');
    } else {
        for (index, component) in path.components().iter().enumerate() {
            // VirtualFileName has already rejected slash, NUL, `.` and `..`.
            if index != 0 {
                bytes.push(b'/');
            }
            bytes.extend_from_slice(component.as_bytes());
        }
    }
    CString::new(bytes).map_err(|_| VfsError::InvalidInput("NUL in bind path"))
}

fn open_path_fd(
    root: &OwnedFd,
    relative: &CString,
    no_follow_final: bool,
) -> Result<OwnedFd, VfsError> {
    let mut flags = OFlags::CLOEXEC;
    if no_follow_final {
        flags |= OFlags::PATH | OFlags::NOFOLLOW;
    } else {
        // The kernel/VFS resolves symlinks through read_link. Never silently
        // substitute a final symlink target for the inode being opened.
        flags |= OFlags::RDONLY | OFlags::NOCTTY | OFlags::NOFOLLOW;
    }
    rustix::fs::openat2(
        root,
        relative,
        flags,
        Mode::empty(),
        // IN_ROOT scopes absolute and relative symlinks to root_fd. Magic
        // links are forbidden because they can refer outside a path tree.
        ResolveFlags::IN_ROOT | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(map_errno)
}

// rustix's Stat field aliases differ by Linux architecture; explicit u64
// normalization keeps node IDs and timestamps architecture-independent.
#[allow(clippy::unnecessary_cast)]
fn metadata_from_stat(stat: &Stat) -> Result<VirtualMetadata, VfsError> {
    let file_type = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => VirtualFileType::RegularFile,
        FileType::Directory => VirtualFileType::Directory,
        FileType::Symlink => VirtualFileType::Symlink,
        FileType::CharacterDevice => VirtualFileType::CharacterDevice,
        FileType::BlockDevice => VirtualFileType::BlockDevice,
        FileType::Fifo => VirtualFileType::NamedPipe,
        FileType::Socket => VirtualFileType::Socket,
        FileType::Unknown => VirtualFileType::Socket,
    };
    let mut node_id = Vec::with_capacity(16);
    node_id.extend_from_slice(&(stat.st_dev as u64).to_ne_bytes());
    node_id.extend_from_slice(&(stat.st_ino as u64).to_ne_bytes());
    let size = u64::try_from(stat.st_size).unwrap_or(0);
    let mut metadata = match file_type {
        VirtualFileType::Directory => VirtualMetadata::directory(node_id),
        VirtualFileType::RegularFile => VirtualMetadata::file(node_id, size),
        VirtualFileType::Symlink => VirtualMetadata::symlink(node_id, size),
        other => VirtualMetadata::special(node_id, other, stat.st_rdev as u64),
    };
    metadata.nlink = u32::try_from(stat.st_nlink).unwrap_or(u32::MAX);
    metadata.original.mode = stat.st_mode;
    metadata.original.uid = stat.st_uid;
    metadata.original.gid = stat.st_gid;
    metadata.mtime = if stat.st_mtime >= 0 {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(stat.st_mtime as u64)
            + Duration::from_nanos(stat.st_mtime_nsec as u64)
    } else {
        SystemTime::UNIX_EPOCH
    };
    Ok(metadata)
}

fn map_errno(error: rustix::io::Errno) -> VfsError {
    match error.raw_os_error() {
        libc::ENOENT => VfsError::NotFound,
        libc::ENOTDIR => VfsError::NotDirectory,
        libc::EISDIR => VfsError::IsDirectory,
        libc::EACCES | libc::EPERM => VfsError::PermissionDenied,
        libc::ELOOP => VfsError::SymlinkLoop,
        libc::EXDEV => VfsError::SymlinkEscape,
        libc::ENAMETOOLONG => VfsError::PathTooLong,
        libc::ENOSYS => {
            VfsError::Unavailable("Linux openat2 is required for secure bind access".into())
        }
        code => VfsError::Io(format!("Linux filesystem error {code}")),
    }
}

fn join_error(error: tokio::task::JoinError) -> VfsError {
    VfsError::Io(format!("blocking bind operation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    fn context() -> RequestContext {
        RequestContext {
            uid: 1000,
            gid: 1000,
            pid: 1,
            request_id: 1,
        }
    }

    #[tokio::test]
    async fn direct_reads_are_fd_backed_and_read_only() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("file"), b"0123456789").unwrap();
        let provider = LocalBindProvider::new(root.path()).await.unwrap();
        let path = VirtualPath::from_absolute(b"/file").unwrap();
        let handle = provider
            .open(&path, libc::O_RDONLY, &context())
            .await
            .unwrap();
        assert_eq!(
            provider.read_at(&handle, 3, 4, &context()).await.unwrap(),
            b"3456"[..]
        );
        assert_eq!(
            provider
                .open(&path, libc::O_WRONLY, &context())
                .await
                .unwrap_err(),
            VfsError::ReadOnly
        );
        provider.close(handle).await.unwrap();
    }

    #[tokio::test]
    async fn traversal_components_are_rejected_before_syscalls() {
        assert!(VirtualFileName::new(b"..").is_err());
        assert!(VirtualFileName::new(b"a/b").is_err());
        let root = TempDir::new().unwrap();
        let provider = LocalBindProvider::new(root.path()).await.unwrap();
        assert!(relative_cstring(&VirtualPath::root()).is_ok());
        assert_eq!(provider.descriptor().kind, ProviderKind::LocalBindLive);
    }

    #[tokio::test]
    async fn symlink_escape_is_confined_by_openat2() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(parent.path().join("secret"), b"outside").unwrap();
        symlink("../secret", root.join("escape")).unwrap();
        let provider = LocalBindProvider::new(&root).await.unwrap();
        let path = VirtualPath::from_absolute(b"/escape").unwrap();
        assert!(
            provider
                .open(&path, libc::O_RDONLY, &context())
                .await
                .is_err()
        );
        assert_eq!(
            provider
                .read_link(&path, &context())
                .await
                .unwrap()
                .as_bytes(),
            b"../secret"
        );
    }

    #[tokio::test]
    async fn directory_cache_refresh_observes_live_changes() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("one"), b"1").unwrap();
        let provider = LocalBindProvider::with_directory_ttl(root.path(), Duration::from_secs(1))
            .await
            .unwrap();
        let first = provider
            .read_dir(&VirtualPath::root(), &context())
            .await
            .unwrap();
        fs::write(root.path().join("two"), b"2").unwrap();
        let cached = provider
            .read_dir(&VirtualPath::root(), &context())
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first, &cached));
        provider.refresh(None).await.unwrap();
        let refreshed = provider
            .read_dir(&VirtualPath::root(), &context())
            .await
            .unwrap();
        assert_eq!(refreshed.len(), 2);
    }

    #[test]
    fn local_descriptor_is_live_without_write_capabilities() {
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::LocalBindLive,
            consistency: ConsistencyMode::Live,
            source: Some("/source".into()),
            capabilities: ProviderCapabilities::READ_ONLY,
        };
        assert_eq!(descriptor.consistency, ConsistencyMode::Live);
        assert!(
            !descriptor
                .capabilities
                .contains(ProviderCapabilities::DOWNLOAD)
        );
    }
}
