//! Operation-time Docker Container Archive API provider.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bollard::query_parameters::DownloadFromContainerOptionsBuilder;
use bytes::Bytes;
use futures_util::StreamExt;
use tuxstack_vfs::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualMetadata, VirtualPath, VirtualPathBytes, is_read_only_open,
};

use crate::client::DockerClient;

use super::spool::{ContentBacking, ContentSpool};
use super::tar_index::{
    ArchiveByteStream, TarEntry, TarEntryKind, TarIndex, TarLimits, TarPath, TarStreamReader,
};

#[async_trait]
pub trait ContainerArchiveSource: Send + Sync {
    async fn archive(
        &self,
        object_id: &str,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<ArchiveByteStream, VfsError>;
}

/// Bollard implementation. It uses the Engine API directly; no CLI, graph
/// driver path, privileged helper, or socket mount is involved.
pub struct DockerContainerArchiveSource {
    client: Arc<DockerClient>,
}

impl DockerContainerArchiveSource {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ContainerArchiveSource for DockerContainerArchiveSource {
    async fn archive(
        &self,
        object_id: &str,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<ArchiveByteStream, VfsError> {
        validate_object_id(object_id)?;
        let path = virtual_path_string(path)?;
        let options = DownloadFromContainerOptionsBuilder::default()
            .path(&path)
            .build();
        let docker = self
            .client
            .inner()
            .clone()
            .with_timeout(self.client.config().request_timeout);
        let object_id = object_id.to_owned();
        let stream = docker
            .download_from_container(&object_id, Some(options))
            .map(move |item| item.map_err(|error| map_archive_error(&error, &object_id, &path)));
        Ok(Box::pin(stream))
    }
}

pub struct ContainerArchiveProvider {
    object_id: String,
    source: Arc<dyn ContainerArchiveSource>,
    spool: ContentSpool,
    limits: TarLimits,
    operation_timeout: Duration,
    provider_kind: ProviderKind,
    source_label: Option<String>,
    next_handle: AtomicU64,
    handles: Mutex<HashMap<u64, ContentBacking>>,
}

impl ContainerArchiveProvider {
    pub fn new(
        object_id: impl Into<String>,
        source: Arc<dyn ContainerArchiveSource>,
        spool: ContentSpool,
        limits: TarLimits,
        operation_timeout: Duration,
    ) -> Result<Self, VfsError> {
        Self::with_kind(
            object_id,
            source,
            spool,
            limits,
            operation_timeout,
            ProviderKind::ContainerArchiveLive,
            None,
        )
    }

    pub fn with_kind(
        object_id: impl Into<String>,
        source: Arc<dyn ContainerArchiveSource>,
        spool: ContentSpool,
        limits: TarLimits,
        operation_timeout: Duration,
        provider_kind: ProviderKind,
        source_label: Option<String>,
    ) -> Result<Self, VfsError> {
        let object_id = object_id.into();
        validate_object_id(&object_id)?;
        limits.validate()?;
        if operation_timeout.is_zero() {
            return Err(VfsError::InvalidInput("archive timeout must be non-zero"));
        }
        Ok(Self {
            object_id,
            source,
            spool,
            limits,
            operation_timeout,
            provider_kind,
            source_label,
            next_handle: AtomicU64::new(1),
            handles: Mutex::new(HashMap::new()),
        })
    }

    async fn operation_index(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<TarIndex, VfsError> {
        tokio::time::timeout(self.operation_timeout, async {
            let stream = self.source.archive(&self.object_id, path, context).await?;
            TarIndex::from_stream(stream, self.limits.clone()).await
        })
        .await
        .map_err(|_| VfsError::TimedOut)?
    }

    async fn operation_content(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<ContentBacking, VfsError> {
        tokio::time::timeout(self.operation_timeout, async {
            let stream = self.source.archive(&self.object_id, path, context).await?;
            extract_regular_file(stream, path, &self.limits, &self.spool).await
        })
        .await
        .map_err(|_| VfsError::TimedOut)?
    }

    fn identity_prefix(&self, path: &VirtualPath) -> Vec<u8> {
        let mut identity = self.object_id.as_bytes().to_vec();
        identity.push(0);
        identity.extend_from_slice(&path.as_bytes());
        identity
    }

    fn metadata_for(
        &self,
        index: &TarIndex,
        entry: &TarEntry,
        path: &VirtualPath,
    ) -> Result<VirtualMetadata, VfsError> {
        index.metadata(&entry.path, &self.identity_prefix(path), 0)
    }

    fn insert_handle(&self, path: &VirtualPath, backing: ContentBacking) -> ProviderFileHandle {
        let id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.handles
            .lock()
            .expect("archive handle lock poisoned")
            .insert(id, backing);
        ProviderFileHandle {
            id,
            path: path.clone(),
            content_generation: 0,
        }
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for ContainerArchiveProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            kind: self.provider_kind,
            consistency: ConsistencyMode::OperationTimeRead,
            source: self.source_label.clone(),
            capabilities: ProviderCapabilities::READ_ONLY,
        }
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        let path = parent.join(name)?;
        self.getattr(&path, context).await
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        if path.is_root() {
            return Ok(VirtualMetadata::directory(self.identity_prefix(path)));
        }
        let index = self.operation_index(path, context).await?;
        let entry = select_archive_root(&index, path)?;
        self.metadata_for(&index, entry, path)
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        let index = self.operation_index(path, context).await?;
        let archive_root = select_archive_root(&index, path)?;
        if archive_root.kind != TarEntryKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        let mut entries = Vec::new();
        for child in index.children(&archive_root.path) {
            let name = VirtualFileName::new(child.path.name().ok_or(VfsError::NotFound)?)?;
            let child_path = path.join(&name)?;
            entries.push(VirtualDirectoryEntry {
                name,
                metadata: self.metadata_for(&index, child, &child_path)?,
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Arc::new(entries))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        let index = self.operation_index(path, context).await?;
        let entry = select_archive_root(&index, path)?;
        if entry.kind != TarEntryKind::Symlink {
            return Err(VfsError::InvalidInput("node is not a symlink"));
        }
        VirtualPathBytes::new(entry.link_target.as_deref().unwrap_or_default())
    }

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        context: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        if !is_read_only_open(flags) {
            return Err(VfsError::ReadOnly);
        }
        let backing = self.operation_content(path, context).await?;
        Ok(self.insert_handle(path, backing))
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        _context: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        let backing = self
            .handles
            .lock()
            .expect("archive handle lock poisoned")
            .get(&handle.id)
            .cloned()
            .ok_or(VfsError::BadHandle)?;
        backing.read_at(offset, size).await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        self.handles
            .lock()
            .expect("archive handle lock poisoned")
            .remove(&handle.id)
            .map(drop)
            .ok_or(VfsError::BadHandle)
    }

    async fn refresh(&self, _path: Option<&VirtualPath>) -> Result<(), VfsError> {
        Ok(())
    }
}

fn select_archive_root<'a>(
    index: &'a TarIndex,
    requested: &VirtualPath,
) -> Result<&'a TarEntry, VfsError> {
    if requested.is_root() {
        return index.get(&TarPath::root()).ok_or(VfsError::NotFound);
    }
    let requested_path = TarPath::from_virtual(requested);
    if let Some(entry) = index.get(&requested_path) {
        return Ok(entry);
    }
    let requested_name = requested.file_name().ok_or(VfsError::NotFound)?.as_bytes();
    let mut candidates = index
        .entries()
        .filter(|entry| entry.path.depth() == 1 && entry.path.name() == Some(requested_name));
    let entry = candidates.next().ok_or(VfsError::NotFound)?;
    if candidates.next().is_some() {
        return Err(VfsError::Io("ambiguous Docker archive root".into()));
    }
    Ok(entry)
}

async fn extract_regular_file(
    stream: ArchiveByteStream,
    requested: &VirtualPath,
    limits: &TarLimits,
    spool: &ContentSpool,
) -> Result<ContentBacking, VfsError> {
    let mut reader = TarStreamReader::new(stream, limits.clone());
    let requested_path = TarPath::from_virtual(requested);
    let requested_name = requested
        .file_name()
        .ok_or(VfsError::IsDirectory)?
        .as_bytes();
    let mut result = None;
    let mut entries = 0usize;

    while let Some(entry) = reader.next_entry().await? {
        entries += 1;
        if entries > limits.max_entries {
            return Err(VfsError::Unavailable("tar entry limit exceeded".into()));
        }
        let matches = entry.path == requested_path
            || (entry.path.depth() == 1 && entry.path.name() == Some(requested_name));
        if !matches {
            reader.skip_entry_body().await?;
            continue;
        }
        if result.is_some() {
            return Err(VfsError::Io("ambiguous Docker file archive".into()));
        }
        if entry.kind == TarEntryKind::Directory {
            return Err(VfsError::IsDirectory);
        }
        if entry.kind != TarEntryKind::RegularFile {
            return Err(
                if matches!(
                    entry.kind,
                    TarEntryKind::CharacterDevice
                        | TarEntryKind::BlockDevice
                        | TarEntryKind::NamedPipe
                        | TarEntryKind::Other
                ) {
                    VfsError::SpecialFile
                } else {
                    VfsError::InvalidInput("archive entry is not a regular file")
                },
            );
        }
        let mut writer = spool.writer();
        while let Some(chunk) = reader.read_body_chunk(128 * 1024).await? {
            writer.push(chunk).await?;
        }
        reader.complete_consumed_body(entry.size).await?;
        result = Some(writer.finish().await?);
    }
    result.ok_or(VfsError::NotFound)
}

fn map_archive_error(error: &bollard::errors::Error, object_id: &str, path: &str) -> VfsError {
    match error {
        bollard::errors::Error::DockerResponseServerError {
            status_code: 401 | 403,
            ..
        } => VfsError::PermissionDenied,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        } => {
            tracing::debug!(
                container_id = %object_id,
                path,
                "Docker Container Archive path is unavailable"
            );
            VfsError::Unsupported(
                "Docker Container Archive does not expose this runtime mount path".into(),
            )
        }
        _ => VfsError::Unavailable(error.to_string()),
    }
}

fn validate_object_id(object_id: &str) -> Result<(), VfsError> {
    if object_id.trim().is_empty() || object_id.as_bytes().contains(&0) {
        Err(VfsError::InvalidInput(
            "empty or NUL-containing Docker object ID",
        ))
    } else {
        Ok(())
    }
}

fn virtual_path_string(path: &VirtualPath) -> Result<String, VfsError> {
    String::from_utf8(path.as_bytes())
        .map_err(|_| VfsError::InvalidInput("Docker Archive API path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures_util::stream;

    use super::*;

    struct FakeSource {
        archive: Vec<u8>,
        paths: Mutex<Vec<Vec<u8>>>,
    }

    #[async_trait]
    impl ContainerArchiveSource for FakeSource {
        async fn archive(
            &self,
            _object_id: &str,
            path: &VirtualPath,
            _context: &RequestContext,
        ) -> Result<ArchiveByteStream, VfsError> {
            self.paths.lock().unwrap().push(path.as_bytes());
            Ok(Box::pin(stream::iter([Ok(Bytes::from(
                self.archive.clone(),
            ))])))
        }
    }

    fn tar_file(name: &[u8], body: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name);
        octal(&mut header[100..108], 0o644);
        octal(&mut header[108..116], 0);
        octal(&mut header[116..124], 0);
        octal(&mut header[124..136], body.len() as u64);
        octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = b'0';
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
        octal(&mut header[148..156], checksum);
        let mut tar = header.to_vec();
        tar.extend_from_slice(body);
        tar.resize(tar.len() + (512 - body.len() % 512) % 512 + 1024, 0);
        tar
    }

    fn octal(field: &mut [u8], value: u64) {
        let text = format!("{:0width$o}", value, width = field.len() - 1);
        field[..text.len()].copy_from_slice(text.as_bytes());
        field[text.len()] = 0;
    }

    fn context() -> RequestContext {
        RequestContext {
            uid: 1,
            gid: 1,
            pid: 1,
            request_id: 1,
        }
    }

    #[tokio::test]
    async fn open_spools_once_and_read_at_is_random() {
        let directory = tempfile::tempdir().unwrap();
        let source = Arc::new(FakeSource {
            archive: tar_file(b"file", b"abcdefgh"),
            paths: Mutex::new(Vec::new()),
        });
        let provider = ContainerArchiveProvider::new(
            "immutable-container-id",
            source.clone(),
            ContentSpool::new(directory.path(), Default::default())
                .await
                .unwrap(),
            TarLimits::default(),
            Duration::from_secs(1),
        )
        .unwrap();
        let path = VirtualPath::from_absolute(b"/file").unwrap();
        let handle = provider.open(&path, 0, &context()).await.unwrap();
        assert_eq!(
            provider.read_at(&handle, 4, 3, &context()).await.unwrap(),
            "efg"
        );
        assert_eq!(
            provider.read_at(&handle, 1, 2, &context()).await.unwrap(),
            "bc"
        );
        assert_eq!(source.paths.lock().unwrap().len(), 1);
        provider.close(handle).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_write_open_without_contacting_source() {
        let directory = tempfile::tempdir().unwrap();
        let source = Arc::new(FakeSource {
            archive: Vec::new(),
            paths: Mutex::new(Vec::new()),
        });
        let provider = ContainerArchiveProvider::new(
            "container-id",
            source.clone(),
            ContentSpool::new(directory.path(), Default::default())
                .await
                .unwrap(),
            TarLimits::default(),
            Duration::from_secs(1),
        )
        .unwrap();
        let error = provider
            .open(
                &VirtualPath::from_absolute(b"/file").unwrap(),
                1, // Linux O_WRONLY
                &context(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, VfsError::ReadOnly);
        assert!(source.paths.lock().unwrap().is_empty());
    }
}
