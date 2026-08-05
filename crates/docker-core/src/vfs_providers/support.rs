//! Private support shared by the volume and helper-bind VFS providers.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tuxstack_vfs::{
    ProviderDescriptor, ProviderFileHandle, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualFileType, VirtualMetadata, VirtualPath, VirtualPathBytes,
};

use crate::{
    FilesystemEntry, FilesystemEntryType, FilesystemError, FilesystemPathToken, FilesystemService,
    FilesystemSession, ListDirectoryRequest, PreviewRequest, StatRequest,
};

pub(crate) const DEFAULT_DIRECTORY_TTL: Duration = Duration::from_secs(2);
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
const MAX_REMOTE_READ_BYTES: u32 = 16 * 1024 * 1024;

#[async_trait]
pub(crate) trait SessionFactory: Send + Sync {
    async fn start(
        &self,
        cancellation: CancellationToken,
    ) -> Result<FilesystemSession, FilesystemError>;
}

struct CachedDirectory {
    inserted: Instant,
    generation: u64,
    entries: Arc<Vec<VirtualDirectoryEntry>>,
}

#[derive(Clone)]
struct OpenRemoteFile {
    path: VirtualPath,
    generation: u64,
}

/// Adapter around the already-existing `FilesystemService`. It deliberately
/// owns no Docker client/backend of its own.
pub(crate) struct HelperProviderCore {
    descriptor: ProviderDescriptor,
    node_namespace: Vec<u8>,
    service: Arc<FilesystemService>,
    factory: Arc<dyn SessionFactory>,
    session: Mutex<Option<FilesystemSession>>,
    directories: RwLock<HashMap<VirtualPath, CachedDirectory>>,
    handles: RwLock<HashMap<u64, OpenRemoteFile>>,
    next_handle: AtomicU64,
    generation: AtomicU64,
    session_epoch: AtomicU64,
    active: AtomicBool,
    directory_ttl: Duration,
}

impl HelperProviderCore {
    pub(crate) fn new(
        descriptor: ProviderDescriptor,
        node_namespace: Vec<u8>,
        service: Arc<FilesystemService>,
        factory: Arc<dyn SessionFactory>,
        directory_ttl: Duration,
    ) -> Self {
        debug_assert!((Duration::from_secs(1)..=Duration::from_secs(5)).contains(&directory_ttl));
        Self {
            descriptor,
            node_namespace,
            service,
            factory,
            session: Mutex::new(None),
            directories: RwLock::new(HashMap::new()),
            handles: RwLock::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            session_epoch: AtomicU64::new(1),
            active: AtomicBool::new(true),
            directory_ttl,
        }
    }

    pub(crate) fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    async fn session(&self) -> Result<FilesystemSession, VfsError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(VfsError::Stale);
        }
        let mut slot = self.session.lock().await;
        if let Some(session) = slot.as_ref() {
            return Ok(session.clone());
        }
        let session = self
            .factory
            .start(CancellationToken::new())
            .await
            .map_err(map_filesystem_error)?;
        if !session.read_only {
            let _ = self.service.stop_session(&session).await;
            return Err(VfsError::ReadOnly);
        }
        *slot = Some(session.clone());
        Ok(session)
    }

    async fn invalidate_failed_session(&self, failed: &FilesystemSession, error: &FilesystemError) {
        if !error.invalidates_session() {
            return;
        }
        let removed = {
            let mut slot = self.session.lock().await;
            if slot.as_ref().map(|current| current.container_id.as_str())
                == Some(failed.container_id.as_str())
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(session) = removed {
            // Precise cleanup: remove only the container ID created for this session.
            let _ = self.service.stop_session(&session).await;
            self.generation.fetch_add(1, Ordering::AcqRel);
            self.session_epoch.fetch_add(1, Ordering::AcqRel);
            self.directories.write().await.clear();
        }
    }

    async fn run<T, F, Fut>(&self, operation: F) -> Result<T, VfsError>
    where
        F: FnOnce(Arc<FilesystemService>, FilesystemSession, CancellationToken) -> Fut,
        Fut: Future<Output = Result<T, FilesystemError>>,
    {
        let session = self.session().await?;
        let service = Arc::clone(&self.service);
        let result = operation(service, session.clone(), CancellationToken::new()).await;
        if let Err(error) = &result {
            self.invalidate_failed_session(&session, error).await;
        }
        result.map_err(map_filesystem_error)
    }

    pub(crate) async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
    ) -> Result<VirtualMetadata, VfsError> {
        self.getattr(&parent.join(name)?).await
    }

    pub(crate) async fn getattr(&self, path: &VirtualPath) -> Result<VirtualMetadata, VfsError> {
        if path.is_root() {
            return Ok(self.root_metadata());
        }
        let token = path_token(path)?;
        let entry = self
            .run(move |service, session, cancellation| async move {
                service
                    .stat(&session, &StatRequest { path_token: token }, cancellation)
                    .await
            })
            .await?;
        Ok(self.metadata(path, &entry))
    }

    pub(crate) async fn read_dir(
        &self,
        path: &VirtualPath,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        let generation = self.generation.load(Ordering::Acquire);
        if let Some(hit) = self.directories.read().await.get(path) {
            if hit.generation == generation && hit.inserted.elapsed() < self.directory_ttl {
                return Ok(Arc::clone(&hit.entries));
            }
        }

        let token = path_token(path)?;
        let result = self
            .run(move |service, session, cancellation| async move {
                service
                    .list_directory(
                        &session,
                        &ListDirectoryRequest {
                            path_token: token,
                            show_hidden: true,
                            limit: Some(MAX_DIRECTORY_ENTRIES),
                            cursor: None,
                        },
                        cancellation,
                    )
                    .await
            })
            .await?;
        if result.truncated || result.next_cursor.is_some() {
            return Err(VfsError::Io(
                "helper truncated a VFS directory listing".into(),
            ));
        }

        let mut entries = Vec::with_capacity(result.entries.len());
        for entry in result.entries {
            let name = VirtualFileName::new(&entry.name_raw)?;
            let child = path.join(&name)?;
            entries.push(VirtualDirectoryEntry {
                name,
                metadata: self.metadata(&child, &entry),
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let entries = Arc::new(entries);
        self.directories.write().await.insert(
            path.clone(),
            CachedDirectory {
                inserted: Instant::now(),
                generation,
                entries: Arc::clone(&entries),
            },
        );
        Ok(entries)
    }

    pub(crate) async fn read_link(&self, path: &VirtualPath) -> Result<VirtualPathBytes, VfsError> {
        let token = path_token(path)?;
        let entry = self
            .run(move |service, session, cancellation| async move {
                service
                    .stat(&session, &StatRequest { path_token: token }, cancellation)
                    .await
            })
            .await?;
        if entry.entry_type != FilesystemEntryType::SymbolicLink {
            return Err(VfsError::InvalidInput("node is not a symlink"));
        }
        VirtualPathBytes::new(
            entry
                .symlink_target_raw
                .ok_or_else(|| VfsError::Io("helper stat omitted symlink target".into()))?,
        )
    }

    pub(crate) async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
    ) -> Result<ProviderFileHandle, VfsError> {
        if !tuxstack_vfs::is_read_only_open(flags) {
            return Err(VfsError::ReadOnly);
        }
        let metadata = self.getattr(path).await?;
        match metadata.file_type {
            VirtualFileType::RegularFile => {}
            VirtualFileType::Directory => return Err(VfsError::IsDirectory),
            VirtualFileType::Symlink => {
                return Err(VfsError::InvalidInput("cannot open a symlink"));
            }
            _ => return Err(VfsError::SpecialFile),
        }
        let id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let generation = self.session_epoch.load(Ordering::Acquire);
        self.handles.write().await.insert(
            id,
            OpenRemoteFile {
                path: path.clone(),
                generation,
            },
        );
        Ok(ProviderFileHandle {
            id,
            path: path.clone(),
            content_generation: generation,
        })
    }

    pub(crate) async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
    ) -> Result<Bytes, VfsError> {
        if size > MAX_REMOTE_READ_BYTES {
            return Err(VfsError::InvalidInput("helper read request is too large"));
        }
        if !self.active.load(Ordering::Acquire) {
            return Err(VfsError::Stale);
        }
        let opened = self
            .handles
            .read()
            .await
            .get(&handle.id)
            .cloned()
            .ok_or(VfsError::BadHandle)?;
        if opened.path != handle.path
            || opened.generation != handle.content_generation
            || opened.generation != self.session_epoch.load(Ordering::Acquire)
        {
            return Err(VfsError::Stale);
        }
        let token = path_token(&opened.path)?;
        let result = self
            .run(move |service, session, cancellation| async move {
                service
                    .preview(
                        &session,
                        &PreviewRequest {
                            path_token: token,
                            offset,
                            limit: u64::from(size),
                        },
                        cancellation,
                    )
                    .await
            })
            .await?;

        let mut bytes = BytesMut::with_capacity(size as usize);
        let mut expected_offset = offset;
        for chunk in result.chunks {
            if chunk.offset != expected_offset {
                return Err(VfsError::Io(
                    "helper returned a non-contiguous read range".into(),
                ));
            }
            let decoded = crate::filesystem_decode_base64(&chunk.data_b64)
                .map_err(|error| VfsError::Io(format!("invalid helper content: {error}")))?;
            if bytes.len().saturating_add(decoded.len()) > size as usize {
                return Err(VfsError::Io("helper exceeded requested read range".into()));
            }
            expected_offset = expected_offset.saturating_add(decoded.len() as u64);
            bytes.extend_from_slice(&decoded);
        }
        Ok(bytes.freeze())
    }

    pub(crate) async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        match self.handles.write().await.remove(&handle.id) {
            Some(opened)
                if opened.path == handle.path && opened.generation == handle.content_generation =>
            {
                Ok(())
            }
            Some(_) => Err(VfsError::BadHandle),
            None => Err(VfsError::BadHandle),
        }
    }

    pub(crate) async fn refresh(&self, path: Option<&VirtualPath>) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut directories = self.directories.write().await;
        if let Some(path) = path {
            directories.retain(|cached, _| !cached.starts_with(path) && !path.starts_with(cached));
        } else {
            directories.clear();
        }
    }

    pub(crate) async fn shutdown(&self) -> Result<(), VfsError> {
        self.active.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.session_epoch.fetch_add(1, Ordering::AcqRel);
        self.directories.write().await.clear();
        self.handles.write().await.clear();
        let session = { self.session.lock().await.take() };
        if let Some(session) = session {
            self.service
                .stop_session(&session)
                .await
                .map_err(map_filesystem_error)?;
        }
        Ok(())
    }

    fn root_metadata(&self) -> VirtualMetadata {
        let mut metadata = VirtualMetadata::directory(self.node_id(&VirtualPath::root()));
        metadata.generation = self.generation.load(Ordering::Acquire);
        metadata
    }

    fn metadata(&self, path: &VirtualPath, entry: &FilesystemEntry) -> VirtualMetadata {
        let file_type = match entry.entry_type {
            FilesystemEntryType::Directory => VirtualFileType::Directory,
            FilesystemEntryType::RegularFile => VirtualFileType::RegularFile,
            FilesystemEntryType::SymbolicLink => VirtualFileType::Symlink,
            FilesystemEntryType::Socket => VirtualFileType::Socket,
            FilesystemEntryType::Fifo => VirtualFileType::NamedPipe,
            FilesystemEntryType::BlockDevice => VirtualFileType::BlockDevice,
            FilesystemEntryType::CharacterDevice => VirtualFileType::CharacterDevice,
            FilesystemEntryType::Unknown => VirtualFileType::Socket,
        };
        let mut metadata = match file_type {
            VirtualFileType::Directory => VirtualMetadata::directory(self.node_id(path)),
            VirtualFileType::RegularFile => {
                VirtualMetadata::file(self.node_id(path), entry.size_bytes.unwrap_or(0))
            }
            VirtualFileType::Symlink => VirtualMetadata::symlink(
                self.node_id(path),
                entry
                    .symlink_target_raw
                    .as_ref()
                    .map_or(0, |target| target.len() as u64),
            ),
            other => VirtualMetadata::special(self.node_id(path), other, 0),
        };
        metadata.original.mode = entry.mode.unwrap_or(match file_type {
            VirtualFileType::Directory => 0o755,
            VirtualFileType::Symlink => 0o777,
            _ => 0o644,
        });
        metadata.original.uid = entry.uid.unwrap_or(0);
        metadata.original.gid = entry.gid.unwrap_or(0);
        metadata.mtime = entry
            .modified_at
            .and_then(|time| u64::try_from(time.timestamp()).ok())
            .map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        metadata.generation = self.generation.load(Ordering::Acquire);
        metadata
    }

    fn node_id(&self, path: &VirtualPath) -> Vec<u8> {
        let mut id = Vec::with_capacity(self.node_namespace.len() + path.byte_len() + 1);
        id.extend_from_slice(&self.node_namespace);
        id.push(0);
        id.extend_from_slice(&path.as_bytes());
        id
    }
}

pub(crate) fn path_token(path: &VirtualPath) -> Result<FilesystemPathToken, VfsError> {
    let mut relative = Vec::with_capacity(path.byte_len().saturating_sub(1));
    for (index, component) in path.components().iter().enumerate() {
        if index != 0 {
            relative.push(b'/');
        }
        relative.extend_from_slice(component.as_bytes());
    }
    FilesystemPathToken::encode_relative(&relative)
        .map_err(|_| VfsError::InvalidInput("invalid helper path"))
}

pub(crate) fn map_filesystem_error(error: FilesystemError) -> VfsError {
    match error {
        FilesystemError::SourceNotFound
        | FilesystemError::ImageNotFound(_)
        | FilesystemError::VolumeNotFound(_)
        | FilesystemError::PathNotFound(_) => VfsError::NotFound,
        FilesystemError::NotDirectory(_) => VfsError::NotDirectory,
        FilesystemError::IsDirectory(_) => VfsError::IsDirectory,
        FilesystemError::PermissionDenied(_) => VfsError::PermissionDenied,
        FilesystemError::InvalidPathToken(_) => VfsError::InvalidInput("invalid helper path token"),
        FilesystemError::PathEscapeRejected(_) => VfsError::SymlinkEscape,
        FilesystemError::UnsupportedFileType(_) => VfsError::SpecialFile,
        FilesystemError::Timeout | FilesystemError::OperationTimeout => VfsError::TimedOut,
        FilesystemError::DockerUnavailable
        | FilesystemError::SessionClosed
        | FilesystemError::SessionInvalidated => VfsError::Unavailable(error.to_string()),
        other => VfsError::Io(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_paths_become_valid_opaque_tokens_without_utf8_conversion() {
        let path = VirtualPath::from_components([
            VirtualFileName::new(b"dir").unwrap(),
            VirtualFileName::new(b"bad-\xff").unwrap(),
        ])
        .unwrap();
        let token = path_token(&path).unwrap();
        assert_eq!(token.decode_relative().unwrap(), b"dir/bad-\xff");
    }
}
