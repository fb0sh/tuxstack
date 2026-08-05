//! Immutable image rootfs provider built from never-started inspection containers.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, DownloadFromContainerOptionsBuilder, RemoveContainerOptions,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tuxstack_vfs::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualMetadata, VirtualPath, VirtualPathBytes, is_read_only_open,
};

use crate::client::DockerClient;

use super::archive::{ContainerArchiveProvider, ContainerArchiveSource};
use super::spool::ContentSpool;
use super::tar_index::{ArchiveByteStream, TarEntryKind, TarIndex, TarLimits};

const CACHE_FORMAT_VERSION: u32 = 1;
const LABEL_MANAGED: &str = "io.github.tuxstack.managed";
const LABEL_PURPOSE: &str = "io.github.tuxstack.purpose";
const LABEL_IMAGE_ID: &str = "io.github.tuxstack.image-id";
const PURPOSE_INDEX: &str = "image-index";
const PURPOSE_CONTENT: &str = "image-content";

#[async_trait]
pub trait ImageInspectionSource: Send + Sync {
    async fn export_created_container(&self, image_id: &str)
    -> Result<ArchiveByteStream, VfsError>;
}

/// Docker source that creates a container in Created state and never invokes
/// start, ENTRYPOINT, CMD, exec, or any image-provided program.
pub struct DockerImageInspectionSource {
    client: Arc<DockerClient>,
}

impl DockerImageInspectionSource {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    async fn create_stopped_container(
        &self,
        image_id: &str,
        purpose: &str,
    ) -> Result<String, VfsError> {
        validate_image_id(image_id)?;
        let labels = HashMap::from([
            (LABEL_MANAGED.to_string(), "true".to_string()),
            (LABEL_PURPOSE.to_string(), purpose.to_string()),
            (LABEL_IMAGE_ID.to_string(), image_id.to_string()),
        ]);
        let name = format!("tuxstack-{purpose}-{}", uuid::Uuid::new_v4());
        let response = self
            .client
            .inner()
            .clone()
            .with_timeout(self.client.config().request_timeout)
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some(image_id.to_string()),
                    labels: Some(labels),
                    network_disabled: Some(true),
                    host_config: Some(HostConfig {
                        network_mode: Some("none".into()),
                        readonly_rootfs: Some(true),
                        cap_drop: Some(vec!["ALL".into()]),
                        security_opt: Some(vec!["no-new-privileges:true".into()]),
                        auto_remove: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| VfsError::Unavailable(error.to_string()))?;
        Ok(response.id)
    }

    fn cleanup_stream(&self, container_id: String, stream: ArchiveByteStream) -> ArchiveByteStream {
        Box::pin(RemoveContainerStream::new(
            stream,
            self.client.inner().clone(),
            container_id,
        ))
    }
}

#[async_trait]
impl ImageInspectionSource for DockerImageInspectionSource {
    async fn export_created_container(
        &self,
        image_id: &str,
    ) -> Result<ArchiveByteStream, VfsError> {
        let container_id = self
            .create_stopped_container(image_id, PURPOSE_INDEX)
            .await?;
        let stream = self
            .client
            .inner()
            .clone()
            .with_timeout(self.client.config().request_timeout)
            .export_container(&container_id)
            .map(|item| item.map_err(|error| VfsError::Unavailable(error.to_string())));
        Ok(self.cleanup_stream(container_id, Box::pin(stream)))
    }
}

#[async_trait]
impl ContainerArchiveSource for DockerImageInspectionSource {
    async fn archive(
        &self,
        image_id: &str,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<ArchiveByteStream, VfsError> {
        let container_id = self
            .create_stopped_container(image_id, PURPOSE_CONTENT)
            .await?;
        let path = String::from_utf8(path.as_bytes())
            .map_err(|_| VfsError::InvalidInput("Docker Archive API path is not UTF-8"))?;
        let options = DownloadFromContainerOptionsBuilder::default()
            .path(&path)
            .build();
        let stream = self
            .client
            .inner()
            .clone()
            .with_timeout(self.client.config().request_timeout)
            .download_from_container(&container_id, Some(options))
            .map(|item| item.map_err(|error| VfsError::Unavailable(error.to_string())));
        Ok(self.cleanup_stream(container_id, Box::pin(stream)))
    }
}

struct RemoveContainerStream {
    inner: ArchiveByteStream,
    docker: bollard::Docker,
    container_id: Option<String>,
    cleanup: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl RemoveContainerStream {
    fn new(inner: ArchiveByteStream, docker: bollard::Docker, container_id: String) -> Self {
        Self {
            inner,
            docker,
            container_id: Some(container_id),
            cleanup: None,
        }
    }

    fn begin_cleanup(&mut self) {
        if self.cleanup.is_some() || self.container_id.is_none() {
            return;
        }
        let container_id = self.container_id.as_ref().expect("checked above").clone();
        let docker = self.docker.clone();
        self.cleanup = Some(Box::pin(async move {
            let _ = docker
                .remove_container(
                    &container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                )
                .await;
        }));
    }

    fn schedule_drop_cleanup(&mut self) {
        let Some(container_id) = self.container_id.take() else {
            return;
        };
        let docker = self.docker.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = docker
                    .remove_container(
                        &container_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            v: false,
                            link: false,
                        }),
                    )
                    .await;
            });
        }
    }
}

impl Stream for RemoveContainerStream {
    type Item = Result<Bytes, VfsError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(cleanup) = self.cleanup.as_mut() {
            return match cleanup.as_mut().poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(()) => {
                    self.container_id = None;
                    self.cleanup = None;
                    Poll::Ready(None)
                }
            };
        }
        match self.inner.as_mut().poll_next(context) {
            Poll::Ready(None) => {
                self.begin_cleanup();
                self.poll_next(context)
            }
            other => other,
        }
    }
}

impl Drop for RemoveContainerStream {
    fn drop(&mut self) {
        self.cleanup = None;
        self.schedule_drop_cleanup();
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedIndex {
    format_version: u32,
    image_id: String,
    payload_checksum: u64,
    index: TarIndex,
}

pub struct ImageRootfsImmutableProvider {
    image_id: String,
    source: Arc<dyn ImageInspectionSource>,
    content_provider: Arc<ContainerArchiveProvider>,
    cache_directory: PathBuf,
    limits: TarLimits,
    operation_timeout: Duration,
    index: Mutex<Option<Arc<TarIndex>>>,
}

impl ImageRootfsImmutableProvider {
    pub fn new(
        image_id: impl Into<String>,
        source: Arc<dyn ImageInspectionSource>,
        content_provider: Arc<ContainerArchiveProvider>,
        cache_directory: impl Into<PathBuf>,
        limits: TarLimits,
        operation_timeout: Duration,
    ) -> Result<Self, VfsError> {
        let image_id = image_id.into();
        validate_image_id(&image_id)?;
        limits.validate()?;
        if operation_timeout.is_zero() {
            return Err(VfsError::InvalidInput(
                "image index timeout must be non-zero",
            ));
        }
        Ok(Self {
            image_id,
            source,
            content_provider,
            cache_directory: cache_directory.into(),
            limits,
            operation_timeout,
            index: Mutex::new(None),
        })
    }

    /// Convenience wiring for the real Docker backend. The same never-started
    /// inspection source is used for indexing and short-lived content reads.
    pub async fn docker(
        image_id: impl Into<String>,
        client: Arc<DockerClient>,
        spool: ContentSpool,
        cache_directory: impl Into<PathBuf>,
        limits: TarLimits,
        operation_timeout: Duration,
    ) -> Result<Self, VfsError> {
        let image_id = image_id.into();
        let source = Arc::new(DockerImageInspectionSource::new(client));
        let archive = Arc::new(ContainerArchiveProvider::with_kind(
            image_id.clone(),
            source.clone(),
            spool,
            limits.clone(),
            operation_timeout,
            ProviderKind::ImageRootfsImmutable,
            Some(image_id.clone()),
        )?);
        Self::new(
            image_id,
            source,
            archive,
            cache_directory,
            limits,
            operation_timeout,
        )
    }

    pub async fn ensure_index(&self) -> Result<Arc<TarIndex>, VfsError> {
        let mut guard = self.index.lock().await;
        if let Some(index) = guard.as_ref() {
            return Ok(index.clone());
        }
        let index = match self.load_cache().await {
            Ok(Some(index)) => index,
            Ok(None) | Err(_) => self.rebuild_index().await?,
        };
        let index = Arc::new(index);
        *guard = Some(index.clone());
        Ok(index)
    }

    pub async fn rebuild_index(&self) -> Result<TarIndex, VfsError> {
        let index = tokio::time::timeout(self.operation_timeout, async {
            let stream = self.source.export_created_container(&self.image_id).await?;
            TarIndex::from_stream(stream, self.limits.clone()).await
        })
        .await
        .map_err(|_| VfsError::TimedOut)??;
        self.persist_cache(&index).await?;
        Ok(index)
    }

    fn cache_path(&self) -> PathBuf {
        self.cache_directory.join(format!(
            "image-index-{:016x}.json",
            fnv1a64(self.image_id.as_bytes())
        ))
    }

    async fn load_cache(&self) -> Result<Option<TarIndex>, VfsError> {
        let path = self.cache_path();
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let persisted: PersistedIndex = serde_json::from_slice(&bytes)
            .map_err(|error| VfsError::Io(format!("corrupt image index: {error}")))?;
        if persisted.format_version != CACHE_FORMAT_VERSION
            || persisted.image_id != self.image_id
            || persisted.payload_checksum != index_checksum(&persisted.index)?
        {
            return Err(VfsError::Io("corrupt image index envelope".into()));
        }
        persisted.index.validate_persisted(&self.limits)?;
        Ok(Some(persisted.index))
    }

    async fn persist_cache(&self, index: &TarIndex) -> Result<(), VfsError> {
        tokio::fs::create_dir_all(&self.cache_directory).await?;
        let persisted = PersistedIndex {
            format_version: CACHE_FORMAT_VERSION,
            image_id: self.image_id.clone(),
            payload_checksum: index_checksum(index)?,
            index: index.clone(),
        };
        let bytes =
            serde_json::to_vec(&persisted).map_err(|error| VfsError::Io(error.to_string()))?;
        let destination = self.cache_path();
        let temporary = destination.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .await?;
        use tokio::io::AsyncWriteExt;
        if let Err(error) = file.write_all(&bytes).await {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
        file.sync_data().await?;
        drop(file);
        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
        Ok(())
    }

    fn identity_prefix(&self) -> Vec<u8> {
        format!("image:{}", self.image_id).into_bytes()
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for ImageRootfsImmutableProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            kind: ProviderKind::ImageRootfsImmutable,
            consistency: ConsistencyMode::Immutable,
            source: Some(self.image_id.clone()),
            capabilities: ProviderCapabilities::READ_ONLY,
        }
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        _context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        let path = parent.join(name)?;
        let index = self.ensure_index().await?;
        index.metadata(
            &super::tar_index::TarPath::from_virtual(&path),
            &self.identity_prefix(),
            0,
        )
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        let index = self.ensure_index().await?;
        index.metadata(
            &super::tar_index::TarPath::from_virtual(path),
            &self.identity_prefix(),
            0,
        )
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        Ok(Arc::new(self.ensure_index().await?.directory_entries(
            path,
            &self.identity_prefix(),
            0,
        )?))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        let index = self.ensure_index().await?;
        let entry = index.get_virtual(path).ok_or(VfsError::NotFound)?;
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
        let content_path = self.ensure_index().await?.content_path(path)?;
        let mut handle = self
            .content_provider
            .open(&content_path, flags, context)
            .await?;
        handle.path = path.clone();
        Ok(handle)
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        context: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        self.content_provider
            .read_at(handle, offset, size, context)
            .await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        self.content_provider.close(handle).await
    }

    async fn refresh(&self, _path: Option<&VirtualPath>) -> Result<(), VfsError> {
        let rebuilt = Arc::new(self.rebuild_index().await?);
        *self.index.lock().await = Some(rebuilt);
        Ok(())
    }
}

fn index_checksum(index: &TarIndex) -> Result<u64, VfsError> {
    let payload = serde_json::to_vec(index).map_err(|error| VfsError::Io(error.to_string()))?;
    Ok(fnv1a64(&payload))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn validate_image_id(image_id: &str) -> Result<(), VfsError> {
    let digest = image_id.strip_prefix("sha256:");
    if !digest.is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        Err(VfsError::InvalidInput(
            "image identity must be a full immutable sha256 image ID",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures_util::stream;

    use super::*;
    use crate::vfs_providers::spool::ContentSpool;

    struct FakeImageSource {
        tar: Vec<u8>,
        exports: AtomicUsize,
    }

    #[async_trait]
    impl ImageInspectionSource for FakeImageSource {
        async fn export_created_container(
            &self,
            _image_id: &str,
        ) -> Result<ArchiveByteStream, VfsError> {
            self.exports.fetch_add(1, Ordering::Relaxed);
            Ok(Box::pin(stream::iter([Ok(Bytes::from(self.tar.clone()))])))
        }
    }

    struct NoContent;

    #[async_trait]
    impl ContainerArchiveSource for NoContent {
        async fn archive(
            &self,
            _id: &str,
            _path: &VirtualPath,
            _ctx: &RequestContext,
        ) -> Result<ArchiveByteStream, VfsError> {
            Err(VfsError::NotFound)
        }
    }

    fn empty_tar() -> Vec<u8> {
        vec![0; 1024]
    }

    const IMAGE_ID: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    async fn provider(cache: &Path, source: Arc<FakeImageSource>) -> ImageRootfsImmutableProvider {
        let spool_dir = tempfile::tempdir().unwrap().keep();
        let archive = Arc::new(
            ContainerArchiveProvider::new(
                IMAGE_ID,
                Arc::new(NoContent),
                ContentSpool::new(spool_dir, Default::default())
                    .await
                    .unwrap(),
                TarLimits::default(),
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        ImageRootfsImmutableProvider::new(
            IMAGE_ID,
            source,
            archive,
            cache,
            TarLimits::default(),
            Duration::from_secs(1),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn persistent_index_is_reused_by_immutable_image_id() {
        let cache = tempfile::tempdir().unwrap();
        let first_source = Arc::new(FakeImageSource {
            tar: empty_tar(),
            exports: AtomicUsize::new(0),
        });
        provider(cache.path(), first_source.clone())
            .await
            .ensure_index()
            .await
            .unwrap();
        assert_eq!(first_source.exports.load(Ordering::Relaxed), 1);

        let second_source = Arc::new(FakeImageSource {
            tar: Vec::new(),
            exports: AtomicUsize::new(0),
        });
        provider(cache.path(), second_source.clone())
            .await
            .ensure_index()
            .await
            .unwrap();
        assert_eq!(second_source.exports.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn corrupted_cache_is_discarded_and_rebuilt() {
        let cache = tempfile::tempdir().unwrap();
        let initial = Arc::new(FakeImageSource {
            tar: empty_tar(),
            exports: AtomicUsize::new(0),
        });
        let initial_provider = provider(cache.path(), initial).await;
        initial_provider.ensure_index().await.unwrap();
        tokio::fs::write(initial_provider.cache_path(), b"corrupt")
            .await
            .unwrap();

        let source = Arc::new(FakeImageSource {
            tar: empty_tar(),
            exports: AtomicUsize::new(0),
        });
        provider(cache.path(), source.clone())
            .await
            .ensure_index()
            .await
            .unwrap();
        assert_eq!(source.exports.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    #[ignore = "requires Docker and TUXSTACK_TEST_IMAGE_ID"]
    async fn real_docker_image_index_uses_never_started_inspection_container() {
        let image_id = std::env::var("TUXSTACK_TEST_IMAGE_ID").expect("image ID");
        let client = Arc::new(DockerClient::connect_default().expect("Docker connection"));
        let cache = tempfile::tempdir().unwrap();
        let spool = tempfile::tempdir().unwrap();
        let provider = ImageRootfsImmutableProvider::docker(
            image_id,
            client,
            ContentSpool::new(spool.path(), Default::default())
                .await
                .unwrap(),
            cache.path(),
            TarLimits::default(),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert!(
            !provider
                .ensure_index()
                .await
                .expect("image index")
                .is_empty()
        );
    }
}
