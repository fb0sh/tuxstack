//! Container rootfs snapshot provider and unified mount router.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use tokio::sync::{Mutex, RwLock};
use tuxstack_vfs::{
    ConsistencyMode, ContainerPath, ContainerPathRouter, ProviderCapabilities, ProviderDescriptor,
    ProviderFileHandle, ProviderKey, ProviderKind, ReadOnlyFilesystemProvider, RequestContext,
    ResolvedContainerMount, VfsError, VirtualDirectoryEntry, VirtualFileName, VirtualMetadata,
    VirtualPath, VirtualPathBytes, is_read_only_open,
};

use crate::client::DockerClient;

use super::archive::ContainerArchiveProvider;
use super::tar_index::{ArchiveByteStream, TarEntry, TarEntryKind, TarIndex, TarLimits, TarPath};

#[async_trait]
pub trait ContainerExportSource: Send + Sync {
    async fn export(&self, immutable_container_id: &str) -> Result<ArchiveByteStream, VfsError>;
}

pub struct DockerContainerExportSource {
    client: Arc<DockerClient>,
}

impl DockerContainerExportSource {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ContainerExportSource for DockerContainerExportSource {
    async fn export(&self, immutable_container_id: &str) -> Result<ArchiveByteStream, VfsError> {
        validate_identity(immutable_container_id)?;
        let stream = self
            .client
            .inner()
            .clone()
            .with_timeout(self.client.config().request_timeout)
            .export_container(immutable_container_id)
            .map(|item| item.map_err(|error| VfsError::Unavailable(error.to_string())));
        Ok(Box::pin(stream))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotMount {
    pub destination: VirtualPath,
}

impl SnapshotMount {
    pub fn new(destination: VirtualPath) -> Result<Self, VfsError> {
        if destination.is_root() {
            return Err(VfsError::InvalidInput(
                "container root cannot be a mount overlay",
            ));
        }
        Ok(Self { destination })
    }
}

#[derive(Clone, Debug)]
struct SnapshotState {
    index: Arc<TarIndex>,
    captured_at: DateTime<Utc>,
    generation: u64,
}

pub struct ContainerRootfsSnapshotProvider {
    immutable_container_id: String,
    generation: AtomicU64,
    export_source: Arc<dyn ContainerExportSource>,
    content_provider: Arc<ContainerArchiveProvider>,
    limits: TarLimits,
    mounts: Vec<SnapshotMount>,
    operation_timeout: Duration,
    capture_lock: Mutex<()>,
    state: RwLock<Option<SnapshotState>>,
}

impl ContainerRootfsSnapshotProvider {
    pub fn new(
        immutable_container_id: impl Into<String>,
        generation: u64,
        export_source: Arc<dyn ContainerExportSource>,
        content_provider: Arc<ContainerArchiveProvider>,
        limits: TarLimits,
        mounts: Vec<SnapshotMount>,
        operation_timeout: Duration,
    ) -> Result<Self, VfsError> {
        let immutable_container_id = immutable_container_id.into();
        validate_identity(&immutable_container_id)?;
        limits.validate()?;
        if operation_timeout.is_zero() {
            return Err(VfsError::InvalidInput("snapshot timeout must be non-zero"));
        }
        let mut mounts = mounts;
        mounts.sort_by(|left, right| {
            left.destination
                .depth()
                .cmp(&right.destination.depth())
                .then_with(|| {
                    left.destination
                        .as_bytes()
                        .cmp(&right.destination.as_bytes())
                })
        });
        mounts.dedup_by(|left, right| left.destination == right.destination);
        Ok(Self {
            immutable_container_id,
            generation: AtomicU64::new(generation),
            export_source,
            content_provider,
            limits,
            mounts,
            operation_timeout,
            capture_lock: Mutex::new(()),
            state: RwLock::new(None),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub async fn captured_at(&self) -> Option<DateTime<Utc>> {
        self.state
            .read()
            .await
            .as_ref()
            .map(|state| state.captured_at)
    }

    pub async fn capture(&self) -> Result<DateTime<Utc>, VfsError> {
        let _capture = self.capture_lock.lock().await;
        self.capture_generation(self.generation()).await
    }

    async fn capture_generation(&self, generation: u64) -> Result<DateTime<Utc>, VfsError> {
        let mut index = tokio::time::timeout(self.operation_timeout, async {
            let stream = self
                .export_source
                .export(&self.immutable_container_id)
                .await?;
            TarIndex::from_stream(stream, self.limits.clone()).await
        })
        .await
        .map_err(|_| VfsError::TimedOut)??;
        apply_mount_overlays(&mut index, &self.mounts, self.limits.max_entries)?;
        let captured_at = Utc::now();
        let mut state = self.state.write().await;
        self.generation.store(generation, Ordering::Release);
        *state = Some(SnapshotState {
            index: Arc::new(index),
            captured_at,
            generation,
        });
        Ok(captured_at)
    }

    async fn snapshot(&self) -> Result<SnapshotState, VfsError> {
        if let Some(snapshot) = self.state.read().await.clone() {
            return Ok(snapshot);
        }
        let _capture = self.capture_lock.lock().await;
        if let Some(snapshot) = self.state.read().await.clone() {
            return Ok(snapshot);
        }
        self.capture_generation(self.generation()).await?;
        self.state
            .read()
            .await
            .clone()
            .ok_or_else(|| VfsError::Unavailable("container snapshot capture failed".into()))
    }

    fn identity_prefix(&self, generation: u64) -> Vec<u8> {
        format!("container:{}:{generation}", self.immutable_container_id).into_bytes()
    }

    fn metadata(
        &self,
        index: &TarIndex,
        entry: &TarEntry,
        generation: u64,
    ) -> Result<VirtualMetadata, VfsError> {
        index.metadata(&entry.path, &self.identity_prefix(generation), generation)
    }

    fn root_metadata(&self) -> VirtualMetadata {
        let mut metadata = VirtualMetadata::directory(self.identity_prefix(self.generation()));
        metadata.generation = self.generation();
        metadata
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for ContainerRootfsSnapshotProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let captured_at = self
            .state
            .try_read()
            .ok()
            .and_then(|state| state.as_ref().map(|state| state.captured_at));
        ProviderDescriptor {
            kind: ProviderKind::ContainerRootfsSnapshot,
            consistency: captured_at.map_or(ConsistencyMode::Unavailable, |captured_at| {
                ConsistencyMode::Snapshot {
                    captured_at,
                    generation: self.generation(),
                }
            }),
            source: Some(self.immutable_container_id.clone()),
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
        let snapshot = self.snapshot().await?;
        snapshot
            .index
            .get_virtual(&path)
            .ok_or(VfsError::NotFound)
            .and_then(|entry| self.metadata(&snapshot.index, entry, snapshot.generation))
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        if path.is_root() {
            return Ok(self.root_metadata());
        }
        let snapshot = self.snapshot().await?;
        snapshot
            .index
            .get_virtual(path)
            .ok_or(VfsError::NotFound)
            .and_then(|entry| self.metadata(&snapshot.index, entry, snapshot.generation))
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        let snapshot = self.snapshot().await?;
        Ok(Arc::new(snapshot.index.directory_entries(
            path,
            &self.identity_prefix(snapshot.generation),
            snapshot.generation,
        )?))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        _context: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        let snapshot = self.snapshot().await?;
        let entry = snapshot.index.get_virtual(path).ok_or(VfsError::NotFound)?;
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
        let snapshot = self.snapshot().await?;
        let content_path = snapshot.index.content_path(path)?;
        let mut handle = self
            .content_provider
            .open(&content_path, flags, context)
            .await?;
        handle.path = path.clone();
        handle.content_generation = snapshot.generation;
        Ok(handle)
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        context: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        if handle.content_generation != self.generation() {
            return Err(VfsError::Stale);
        }
        self.content_provider
            .read_at(handle, offset, size, context)
            .await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        self.content_provider.close(handle).await
    }

    async fn refresh(&self, _path: Option<&VirtualPath>) -> Result<(), VfsError> {
        let _capture = self.capture_lock.lock().await;
        let next = self
            .generation()
            .checked_add(1)
            .ok_or_else(|| VfsError::Unavailable("snapshot generation exhausted".into()))?;
        self.capture_generation(next).await.map(drop)
    }
}

fn apply_mount_overlays(
    index: &mut TarIndex,
    mounts: &[SnapshotMount],
    max_entries: usize,
) -> Result<(), VfsError> {
    for mount in mounts {
        let path = TarPath::from_virtual(&mount.destination);
        // Exported data under a mounted destination is the hidden lower layer,
        // not mount content. Suppress it before inserting the synthetic route.
        index.remove_descendants(&path);
        let synthetic = TarEntry::synthetic_directory(path.clone());
        if index.get(&path).is_some() {
            index.replace(synthetic);
        } else {
            index.insert_synthetic_directory(path, max_entries)?;
        }
    }
    Ok(())
}

/// Read-only provider that presents a container as one logical tree while
/// dispatching each operation through tuxstack-vfs's component-aware deepest
/// mount router. Synthetic parents remain supplied by the snapshot index.
pub struct ContainerProviderRouter {
    router: ContainerPathRouter,
    rootfs: Arc<ContainerRootfsSnapshotProvider>,
}

impl ContainerProviderRouter {
    pub fn new(
        container_id: impl Into<String>,
        rootfs: Arc<ContainerRootfsSnapshotProvider>,
        mounts: Vec<ResolvedContainerMount>,
    ) -> Result<Self, VfsError> {
        let container_id = container_id.into();
        validate_identity(&container_id)?;
        let rootfs_provider: Arc<dyn ReadOnlyFilesystemProvider> = rootfs.clone();
        let router = ContainerPathRouter::new(
            container_id,
            ProviderKey(format!("rootfs:{}", rootfs.generation())),
            rootfs_provider,
            mounts,
        )?;
        Ok(Self { router, rootfs })
    }

    pub fn router(&self) -> &ContainerPathRouter {
        &self.router
    }

    fn route(&self, path: &VirtualPath) -> Result<tuxstack_vfs::ResolvedRoute, VfsError> {
        let mut route = self.router.route(&ContainerPath(path.clone()))?;
        if route.mount.is_none() {
            route.provider_key = ProviderKey(format!("rootfs:{}", self.rootfs.generation()));
        }
        Ok(route)
    }

    fn is_mount_route_parent(&self, path: &VirtualPath) -> bool {
        self.router.mounts().iter().any(|mount| {
            mount.destination.depth() > path.depth() && mount.destination.starts_with(path)
        })
    }

    fn synthetic_mount_route_metadata(&self, path: &VirtualPath) -> VirtualMetadata {
        let mut metadata = VirtualMetadata::directory(
            [
                b"container-mount-route:".as_slice(),
                path.as_bytes().as_slice(),
            ]
            .concat(),
        );
        metadata.generation = self.rootfs.generation();
        metadata
    }

    async fn merge_mount_route_children(
        &self,
        path: &VirtualPath,
        mut entries: Vec<VirtualDirectoryEntry>,
        context: &RequestContext,
    ) -> Result<Vec<VirtualDirectoryEntry>, VfsError> {
        for mount in self.router.mounts() {
            if mount.destination.depth() <= path.depth() || !mount.destination.starts_with(path) {
                continue;
            }
            let name = mount.destination.components()[path.depth()].clone();
            let child = path.join(&name)?;
            let metadata = if child == mount.destination {
                mount
                    .provider
                    .getattr(&mount.provider_root, context)
                    .await?
            } else {
                self.synthetic_mount_route_metadata(&child)
            };
            if let Some(existing) = entries.iter_mut().find(|entry| entry.name == name) {
                existing.metadata = metadata;
            } else {
                entries.push(VirtualDirectoryEntry { name, metadata });
            }
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for ContainerProviderRouter {
    fn descriptor(&self) -> ProviderDescriptor {
        self.rootfs.descriptor()
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.getattr(&parent.join(name)?, context).await
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        let route = self.route(path)?;
        let is_exact_mount = route
            .mount
            .as_ref()
            .is_some_and(|mount| mount.destination == *path);
        if self.is_mount_route_parent(path) && !is_exact_mount {
            return Ok(self.synthetic_mount_route_metadata(path));
        }
        route.provider.getattr(&route.provider_path, context).await
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        let route = self.route(path)?;
        let entries = match route.provider.read_dir(&route.provider_path, context).await {
            Ok(entries) => Arc::unwrap_or_clone(entries),
            Err(VfsError::NotFound | VfsError::NotDirectory)
                if self.is_mount_route_parent(path) =>
            {
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        Ok(Arc::new(
            self.merge_mount_route_children(path, entries, context)
                .await?,
        ))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        context: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        let route = self.route(path)?;
        route
            .provider
            .read_link(&route.provider_path, context)
            .await
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
        if self.is_mount_route_parent(path)
            && self.getattr(path, context).await?.file_type
                == tuxstack_vfs::VirtualFileType::Directory
        {
            return Err(VfsError::IsDirectory);
        }
        let route = self.route(path)?;
        let mut handle = route
            .provider
            .open(&route.provider_path, flags, context)
            .await?;
        // Preserve the logical path for stable FUSE handle diagnostics. The
        // routed provider keys the actual backing by handle ID.
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
        let route = self.route(&handle.path)?;
        let mut routed_handle = handle.clone();
        routed_handle.path = route.provider_path;
        route
            .provider
            .read_at(&routed_handle, offset, size, context)
            .await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        let route = self.route(&handle.path)?;
        let mut routed_handle = handle;
        routed_handle.path = route.provider_path;
        route.provider.close(routed_handle).await
    }

    async fn refresh(&self, path: Option<&VirtualPath>) -> Result<(), VfsError> {
        match path {
            Some(path) => {
                let route = self.route(path)?;
                route.provider.refresh(Some(&route.provider_path)).await
            }
            None => self.rootfs.refresh(None).await,
        }
    }
}

fn validate_identity(identity: &str) -> Result<(), VfsError> {
    if identity.len() != 64 || !identity.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(VfsError::InvalidInput(
            "container snapshot identity must be a full immutable container ID",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures_util::stream;
    use tuxstack_vfs::InMemoryProvider;

    use super::*;
    use crate::vfs_providers::archive::ContainerArchiveSource;
    use crate::vfs_providers::spool::ContentSpool;

    struct BytesExport(Vec<u8>);

    #[async_trait]
    impl ContainerExportSource for BytesExport {
        async fn export(&self, _id: &str) -> Result<ArchiveByteStream, VfsError> {
            Ok(Box::pin(stream::iter([Ok(Bytes::from(self.0.clone()))])))
        }
    }

    struct EmptyArchive;

    #[async_trait]
    impl ContainerArchiveSource for EmptyArchive {
        async fn archive(
            &self,
            _id: &str,
            _path: &VirtualPath,
            _ctx: &RequestContext,
        ) -> Result<ArchiveByteStream, VfsError> {
            Err(VfsError::NotFound)
        }
    }

    fn tar(entries: &[(&[u8], u8)]) -> Vec<u8> {
        let mut result = Vec::new();
        for (name, kind) in entries {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name);
            octal(
                &mut header[100..108],
                if *kind == b'5' { 0o755 } else { 0o644 },
            );
            octal(&mut header[108..116], 0);
            octal(&mut header[116..124], 0);
            octal(&mut header[124..136], 0);
            octal(&mut header[136..148], 0);
            header[148..156].fill(b' ');
            header[156] = *kind;
            let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
            octal(&mut header[148..156], checksum);
            result.extend_from_slice(&header);
        }
        result.resize(result.len() + 1024, 0);
        result
    }

    fn octal(field: &mut [u8], value: u64) {
        let text = format!("{:0width$o}", value, width = field.len() - 1);
        field[..text.len()].copy_from_slice(text.as_bytes());
        field[text.len()] = 0;
    }

    const CONTAINER_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn context() -> RequestContext {
        RequestContext {
            uid: 1,
            gid: 1,
            pid: 1,
            request_id: 1,
        }
    }

    async fn rootfs(
        export: Vec<u8>,
        mounts: Vec<SnapshotMount>,
    ) -> Arc<ContainerRootfsSnapshotProvider> {
        let directory = tempfile::tempdir().unwrap().keep();
        let archive = Arc::new(
            ContainerArchiveProvider::new(
                CONTAINER_ID,
                Arc::new(EmptyArchive),
                ContentSpool::new(directory, Default::default())
                    .await
                    .unwrap(),
                TarLimits::default(),
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        Arc::new(
            ContainerRootfsSnapshotProvider::new(
                CONTAINER_ID,
                9,
                Arc::new(BytesExport(export)),
                archive,
                TarLimits::default(),
                mounts,
                Duration::from_secs(1),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn overlays_create_parents_and_suppress_shadowed_export_entries() {
        let rootfs = rootfs(
            tar(&[(b"data/lower", b'0'), (b"unrelated", b'0')]),
            vec![
                SnapshotMount::new(VirtualPath::from_absolute(b"/data/mounted/deep").unwrap())
                    .unwrap(),
                SnapshotMount::new(VirtualPath::from_absolute(b"/data").unwrap()).unwrap(),
            ],
        )
        .await;
        rootfs.capture().await.unwrap();
        let descriptor = rootfs.descriptor();
        assert!(matches!(
            descriptor.consistency,
            ConsistencyMode::Snapshot { generation: 9, .. }
        ));
        let root = rootfs
            .read_dir(&VirtualPath::root(), &context())
            .await
            .unwrap();
        assert!(root.iter().any(|entry| entry.name.as_bytes() == b"data"));
        let data = rootfs
            .read_dir(&VirtualPath::from_absolute(b"/data").unwrap(), &context())
            .await
            .unwrap();
        assert!(!data.iter().any(|entry| entry.name.as_bytes() == b"lower"));
    }

    #[tokio::test]
    #[ignore = "requires Docker and TUXSTACK_TEST_CONTAINER_ID"]
    async fn real_docker_snapshot_export_is_indexed_without_starting_helpers() {
        let container_id = std::env::var("TUXSTACK_TEST_CONTAINER_ID").expect("container ID");
        let client = Arc::new(DockerClient::connect_default().expect("Docker connection"));
        let directory = tempfile::tempdir().unwrap();
        let archive = Arc::new(
            ContainerArchiveProvider::new(
                container_id.clone(),
                Arc::new(
                    crate::vfs_providers::archive::DockerContainerArchiveSource::new(
                        client.clone(),
                    ),
                ),
                ContentSpool::new(directory.path(), Default::default())
                    .await
                    .unwrap(),
                TarLimits::default(),
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        let rootfs = ContainerRootfsSnapshotProvider::new(
            container_id,
            1,
            Arc::new(DockerContainerExportSource::new(client)),
            archive,
            TarLimits::default(),
            vec![],
            Duration::from_secs(60),
        )
        .unwrap();
        rootfs.capture().await.expect("snapshot export");
        assert!(matches!(
            rootfs.descriptor().consistency,
            ConsistencyMode::Snapshot { generation: 1, .. }
        ));
    }

    #[tokio::test]
    async fn router_uses_deepest_component_mount_and_not_string_prefix() {
        let rootfs = rootfs(tar(&[(b"app/data2", b'5')]), vec![]).await;
        rootfs.capture().await.unwrap();
        let shallow = Arc::new(InMemoryProvider::new());
        shallow
            .add_file(
                VirtualPath::from_absolute(b"/shallow").unwrap(),
                b"s",
                Bytes::from_static(b"s"),
            )
            .unwrap();
        let deep = Arc::new(InMemoryProvider::new());
        deep.add_file(
            VirtualPath::from_absolute(b"/deep").unwrap(),
            b"d",
            Bytes::from_static(b"d"),
        )
        .unwrap();
        let mounts = vec![
            ResolvedContainerMount {
                destination: VirtualPath::from_absolute(b"/app").unwrap(),
                provider_key: ProviderKey("shallow".into()),
                provider_root: VirtualPath::root(),
                provider: shallow,
            },
            ResolvedContainerMount {
                destination: VirtualPath::from_absolute(b"/app/data").unwrap(),
                provider_key: ProviderKey("deep".into()),
                provider_root: VirtualPath::root(),
                provider: deep,
            },
        ];
        let provider = ContainerProviderRouter::new(CONTAINER_ID, rootfs, mounts).unwrap();
        assert_eq!(
            provider
                .router()
                .route(&ContainerPath(
                    VirtualPath::from_absolute(b"/app/data/deep").unwrap()
                ))
                .unwrap()
                .provider_key
                .0,
            "deep"
        );
        assert_eq!(
            provider
                .router()
                .route(&ContainerPath(
                    VirtualPath::from_absolute(b"/app/data2").unwrap()
                ))
                .unwrap()
                .provider_key
                .0,
            "shallow"
        );
    }
}
