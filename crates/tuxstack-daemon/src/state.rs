use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fuser::BackgroundSession;
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::cache::{
    ChangeKind, ChangeNotification, DefaultEventClassifier, DockerEventMonitor, run_monitor,
};
use tuxstack_docker_core::services::{
    DockerServices, ListContainersOptions, ListImagesOptions, ListVolumesOptions,
};
use tuxstack_docker_core::vfs_providers::{
    ContainerArchiveProvider, ContainerProviderRouter, ContainerRootfsSnapshotProvider,
    ContentSpool, DockerContainerArchiveSource, DockerContainerExportSource, HelperBindProvider,
    ImageRootfsImmutableProvider, LocalBindProvider, NamedVolumeProviderPool, SnapshotMount,
    TarLimits, cleanup_orphan_bind_helpers,
};
use tuxstack_docker_core::{DockerClient, DockerConfig};
use tuxstack_domain::{ContainerDetail, ContainerMountType, ContainerSummary, ImageSummary};
use tuxstack_protocol::{
    ConsistencyMode as WireConsistency, DaemonLifecycle, DaemonStatus, DockerConnectionStatus,
    DockerResourceRef, MountState, MountStatus, ProviderCapabilities as WireCapabilities,
    ProviderDescriptor as WireDescriptor, ProviderKind as WireProviderKind, ProviderStatus,
    ResourceChange, ResourceFusePath, ResourceKind, ResourcePath,
};
use tuxstack_vfs::{
    ConsistencyMode, DockerFilesystemResource, FuseNameCodec, InvalidationNotifier,
    NamespaceProvider, ProviderDescriptor, ProviderKey, ProviderKind, ROOT_INODE,
    ReadOnlyFilesystemProvider, ReadOnlyFuseAdapter, RequestContext, ResolvedContainerMount,
    VfsError, VirtualFileName, VirtualPath, VirtualPathBytes,
};

use crate::config::DaemonPaths;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(60);
const HANDLE_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PROVIDER_OPERATIONS: usize = 16;
const MAX_OPEN_HANDLES: usize = 4096;
const RUNTIME_MOUNTS: [&str; 3] = ["/etc/hosts", "/etc/hostname", "/etc/resolv.conf"];

#[derive(Debug, Clone)]
pub enum DaemonEvent {
    Resource {
        kind: ResourceKind,
        resource: Option<DockerResourceRef>,
        change: ResourceChange,
    },
    Mount(MountStatus),
    Status(DaemonStatus),
}

#[derive(Default)]
struct ProviderRegistries {
    images: HashMap<String, Arc<ImageRootfsImmutableProvider>>,
    containers: HashMap<String, Arc<ContainerProviderRouter>>,
    rootfs: HashMap<String, Arc<ContainerRootfsSnapshotProvider>>,
    archives: Vec<Arc<ContainerArchiveProvider>>,
    local_binds: Vec<Arc<LocalBindProvider>>,
    helper_binds: Vec<Arc<HelperBindProvider>>,
    volume_names: HashSet<String>,
}

struct NamespaceMount {
    path: VirtualPath,
    key: String,
    provider: Arc<dyn ReadOnlyFilesystemProvider>,
}

struct NamespaceAlias {
    path: VirtualPath,
    target: VirtualPathBytes,
}

#[derive(Default)]
struct NamespacePlan {
    mounts: Vec<NamespaceMount>,
    aliases: Vec<NamespaceAlias>,
    registries: ProviderRegistries,
}

pub struct DaemonState {
    pub paths: DaemonPaths,
    pub services: Arc<DockerServices>,
    pub client: Arc<DockerClient>,
    started_at: Instant,
    lifecycle: RwLock<DaemonLifecycle>,
    daemon_id: Option<String>,
    daemon_instance: String,
    mount: Mutex<Option<BackgroundSession>>,
    mount_status: RwLock<MountStatus>,
    notifier: RwLock<Option<InvalidationNotifier>>,
    namespace: Arc<NamespaceProvider>,
    spool: ContentSpool,
    volume_pool: Arc<NamedVolumeProviderPool>,
    providers: RwLock<ProviderRegistries>,
    namespace_rebuild: AsyncMutex<()>,
    snapshot_generation: AtomicU64,
    events: broadcast::Sender<DaemonEvent>,
    shutdown: CancellationToken,
    event_monitor: DockerEventMonitor,
}

impl DaemonState {
    pub async fn start(paths: DaemonPaths) -> Result<Arc<Self>> {
        paths.prepare()?;
        recover_stale_mount(&paths.mount_point)?;
        let client = Arc::new(
            DockerClient::connect_with_config(DockerConfig::default())
                .context("connect to local Docker Engine")?,
        );
        client.ping().await.context("ping Docker Engine")?;
        let endpoint_identity = client.endpoint_fingerprint();
        let daemon_id = Some(endpoint_identity.clone());
        let daemon_instance = format!("uid-{}:{endpoint_identity}", unsafe { libc::geteuid() });
        let services = Arc::new(DockerServices::new(Arc::clone(&client)));
        if let Err(error) = services.filesystem.cleanup_orphan_sessions().await {
            tracing::warn!(%error, "failed to clean orphan filesystem helpers at startup");
        }
        if let Err(error) = cleanup_orphan_bind_helpers(&client, &daemon_instance).await {
            tracing::warn!(%error, "failed to clean exact-label bind helpers at startup");
        }
        let spool = ContentSpool::new(paths.spool_dir.clone(), Default::default())
            .await
            .context("create shared VFS content spool")?;
        let volume_pool = Arc::new(NamedVolumeProviderPool::new(
            endpoint_identity,
            Arc::new(services.filesystem.clone()),
        ));
        let (events, _) = broadcast::channel(512);
        let mut event_monitor = DockerEventMonitor::new();
        event_monitor.rebind(Arc::clone(&client));
        let state = Arc::new(Self {
            paths,
            services,
            client,
            started_at: Instant::now(),
            lifecycle: RwLock::new(DaemonLifecycle::Starting),
            daemon_id,
            daemon_instance,
            mount: Mutex::new(None),
            mount_status: RwLock::new(MountStatus {
                state: MountState::Unmounted,
                mount_point: None,
                read_only: true,
            }),
            notifier: RwLock::new(None),
            namespace: Arc::new(NamespaceProvider::new()),
            spool,
            volume_pool,
            providers: RwLock::new(ProviderRegistries::default()),
            namespace_rebuild: AsyncMutex::new(()),
            snapshot_generation: AtomicU64::new(1),
            events,
            shutdown: CancellationToken::new(),
            event_monitor,
        });
        state.mount().await?;
        state.start_events();
        *write(&state.lifecycle) = DaemonLifecycle::Ready;
        let _ = state.events.send(DaemonEvent::Status(state.status()));
        Ok(state)
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.events.subscribe()
    }

    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            lifecycle: read(&self.lifecycle).clone(),
            docker: DockerConnectionStatus::Connected {
                daemon_id: self.daemon_id.clone(),
            },
            mount: self.mount_status(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
        }
    }

    pub fn mount_status(&self) -> MountStatus {
        read(&self.mount_status).clone()
    }

    pub async fn mount(&self) -> Result<MountStatus> {
        if lock(&self.mount).is_some() {
            return Ok(self.mount_status());
        }
        *write(&self.mount_status) = MountStatus {
            state: MountState::Mounting,
            mount_point: Some(self.paths.mount_point.clone()),
            read_only: true,
        };
        self.rebuild_namespace().await?;
        let provider: Arc<dyn ReadOnlyFilesystemProvider> = self.namespace.clone();
        let adapter = ReadOnlyFuseAdapter::new(
            provider,
            DockerFilesystemResource::Volume {
                volume_name: "namespace".to_owned(),
            },
            self.client.endpoint_fingerprint(),
            "docker-namespace",
            unsafe { libc::geteuid() },
            unsafe { libc::getegid() },
            OPERATION_TIMEOUT,
            MAX_PROVIDER_OPERATIONS,
            MAX_OPEN_HANDLES,
            HANDLE_IDLE_TIMEOUT,
        )?;
        let notifier = adapter.notifier();
        let session = adapter
            .spawn_mount(&self.paths.mount_point)
            .with_context(|| format!("mount {}", self.paths.mount_point.display()))?;
        *write(&self.notifier) = Some(notifier);
        *lock(&self.mount) = Some(session);
        let status = MountStatus {
            state: MountState::Mounted,
            mount_point: Some(self.paths.mount_point.clone()),
            read_only: true,
        };
        *write(&self.mount_status) = status.clone();
        let _ = self.events.send(DaemonEvent::Mount(status.clone()));
        Ok(status)
    }

    pub async fn unmount(&self) -> Result<MountStatus> {
        *write(&self.mount_status) = MountStatus {
            state: MountState::Unmounting,
            mount_point: Some(self.paths.mount_point.clone()),
            read_only: true,
        };
        *write(&self.notifier) = None;
        let session = lock(&self.mount).take();
        if let Some(session) = session {
            tokio::task::spawn_blocking(move || session.umount_and_join())
                .await
                .context("join FUSE unmount task")??;
        }
        let status = MountStatus {
            state: MountState::Unmounted,
            mount_point: None,
            read_only: true,
        };
        *write(&self.mount_status) = status.clone();
        let _ = self.events.send(DaemonEvent::Mount(status.clone()));
        Ok(status)
    }

    pub async fn remount(&self) -> Result<MountStatus> {
        self.unmount().await?;
        self.mount().await
    }

    pub async fn resource_path(&self, resource: DockerResourceRef) -> Result<ResourceFusePath> {
        let namespace_path = resource_namespace_path(&resource)?;
        self.ensure_container_snapshot(&resource, true).await?;
        let registries = read(&self.providers);
        let resolved = self
            .namespace
            .provider_at(&namespace_path)?
            .with_context(|| {
                format!("resource is not attached to the VFS namespace: {resource:?}")
            })?;
        if !resolved.relative_path.is_root() {
            bail!("resource namespace root resolved below a provider root");
        }
        let descriptor = descriptor_for_route(
            &registries,
            &resolved.key,
            &resolved.relative_path,
            &resolved.provider,
        )?;
        Ok(ResourceFusePath {
            resource,
            path: host_namespace_path(&self.paths.mount_point, &namespace_path),
            descriptor: wire_descriptor(descriptor),
        })
    }

    pub async fn provider_descriptor(&self, path: ResourcePath) -> Result<WireDescriptor> {
        self.ensure_container_snapshot(&path.resource, path.components.is_empty())
            .await?;
        let mut namespace_path = resource_namespace_path(&path.resource)?;
        for component in path.components {
            namespace_path = namespace_path.join(&VirtualFileName::new(component.as_bytes())?)?;
        }
        let registries = read(&self.providers);
        let resolved = self
            .namespace
            .provider_at(&namespace_path)?
            .context("resource path is not attached to the VFS namespace")?;
        let descriptor = descriptor_for_route(
            &registries,
            &resolved.key,
            &resolved.relative_path,
            &resolved.provider,
        )?;
        Ok(wire_descriptor(descriptor))
    }

    async fn ensure_container_snapshot(
        &self,
        resource: &DockerResourceRef,
        root_path: bool,
    ) -> Result<()> {
        if !root_path {
            return Ok(());
        }
        let DockerResourceRef::Container { container_id } = resource else {
            return Ok(());
        };
        let rootfs = read(&self.providers).rootfs.get(container_id).cloned();
        let Some(rootfs) = rootfs else {
            bail!("container rootfs provider is not attached: {container_id}");
        };
        if rootfs.captured_at().await.is_none() {
            rootfs
                .capture()
                .await
                .with_context(|| format!("capture container filesystem snapshot {container_id}"))?;
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        *write(&self.lifecycle) = DaemonLifecycle::Stopping;
        let _ = self.events.send(DaemonEvent::Status(self.status()));
        self.shutdown.cancel();
        self.event_monitor.shutdown();
        let _ = self.unmount().await;

        let old = {
            let mut providers = write(&self.providers);
            let _ = self.namespace.clear();
            std::mem::take(&mut *providers)
        };
        shutdown_helper_binds(old.helper_binds).await;
        if let Err(error) = self.volume_pool.shutdown().await {
            tracing::warn!(%error, "failed to stop named-volume providers during shutdown");
        }
        if let Err(error) = self.services.filesystem.cleanup_orphan_sessions().await {
            tracing::warn!(%error, "failed to clean filesystem helpers during shutdown");
        }
        if let Err(error) = cleanup_orphan_bind_helpers(&self.client, &self.daemon_instance).await {
            tracing::warn!(%error, "failed to clean exact-label bind helpers during shutdown");
        }
        if let Err(error) = tokio::fs::remove_dir_all(&self.paths.spool_dir).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "failed to remove VFS content spool during shutdown");
            }
        }
        let _ = std::fs::remove_file(&self.paths.socket_path);
        Ok(())
    }

    async fn rebuild_namespace(&self) -> Result<()> {
        let _rebuild = self.namespace_rebuild.lock().await;
        let plan = self.build_namespace_plan().await;
        let new_volume_names = plan.registries.volume_names.clone();
        let (old_helpers, removed_volumes) = {
            let mut registries = write(&self.providers);
            let old_volume_names = registries.volume_names.clone();
            self.namespace.clear()?;
            for mount in &plan.mounts {
                self.namespace.mount(
                    mount.path.clone(),
                    mount.key.clone(),
                    Arc::clone(&mount.provider),
                )?;
            }
            for alias in &plan.aliases {
                self.namespace
                    .alias(alias.path.clone(), alias.target.clone())?;
            }
            let old = std::mem::replace(&mut *registries, plan.registries);
            let removed = old_volume_names
                .difference(&new_volume_names)
                .cloned()
                .collect::<Vec<_>>();
            (old.helper_binds, removed)
        };
        shutdown_helper_binds(old_helpers).await;
        for volume_name in removed_volumes {
            if let Err(error) = self.volume_pool.remove(&volume_name).await {
                tracing::warn!(%error, volume = %volume_name, "failed to remove volume provider from pool");
            }
        }
        Ok(())
    }

    async fn build_namespace_plan(&self) -> NamespacePlan {
        let mut plan = NamespacePlan::default();
        let container_options = ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let volume_options = ListVolumesOptions::default();
        let (containers, images, volumes) = tokio::join!(
            self.services.containers.list_containers(&container_options),
            self.services
                .images
                .list_images(ListImagesOptions::default()),
            self.services.volumes.list_volumes(&volume_options),
        );

        match volumes {
            Ok(volumes) => {
                for volume in volumes {
                    if let Err(error) = add_volume(&mut plan, &self.volume_pool, &volume.name) {
                        tracing::warn!(%error, volume = %volume.name, "volume VFS provider was not attached");
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "failed to enumerate volume VFS providers"),
        }

        match images {
            Ok(images) => {
                for image in images {
                    if let Err(error) = self.add_image(&mut plan, image).await {
                        tracing::warn!(%error, "image VFS provider was not attached");
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "failed to enumerate image VFS providers"),
        }

        match containers {
            Ok(containers) => {
                for container in containers {
                    if let Err(error) = self.add_container(&mut plan, container).await {
                        tracing::warn!(%error, "container VFS provider was not attached");
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "failed to enumerate container VFS providers"),
        }
        plan
    }

    async fn add_image(&self, plan: &mut NamespacePlan, image: ImageSummary) -> Result<()> {
        let image_id = image.id.clone();
        let provider = Arc::new(
            ImageRootfsImmutableProvider::docker(
                image_id.clone(),
                Arc::clone(&self.client),
                self.spool.clone(),
                self.paths.cache_dir.join("vfs-image-indexes"),
                TarLimits::default(),
                SNAPSHOT_TIMEOUT,
            )
            .await?,
        );
        let path = image_namespace_path(&image_id)?;
        let provider_dyn: Arc<dyn ReadOnlyFilesystemProvider> = provider.clone();
        plan.mounts.push(NamespaceMount {
            path: path.clone(),
            key: format!("image:{image_id}"),
            provider: provider_dyn,
        });
        for tag in image.repo_tags {
            if tag == "<none>:<none>" {
                continue;
            }
            plan.aliases.push(NamespaceAlias {
                path: encoded_path("/images", &tag)?,
                target: VirtualPathBytes::new(path.as_bytes())?,
            });
        }
        plan.registries.images.insert(image_id, provider);
        Ok(())
    }

    async fn add_container(
        &self,
        plan: &mut NamespacePlan,
        summary: ContainerSummary,
    ) -> Result<()> {
        let container_id = summary.id.clone();
        let detail = self
            .services
            .containers
            .inspect_container(&container_id)
            .await
            .with_context(|| format!("inspect container {container_id}"))?;
        let generation = self.snapshot_generation.fetch_add(1, Ordering::Relaxed);
        let archive_source = Arc::new(DockerContainerArchiveSource::new(Arc::clone(&self.client)));
        let root_archive = Arc::new(ContainerArchiveProvider::new(
            container_id.clone(),
            archive_source.clone(),
            self.spool.clone(),
            TarLimits::default(),
            OPERATION_TIMEOUT,
        )?);
        let (mounts, snapshot_mounts) = self.container_mounts(plan, &detail, archive_source).await;
        let rootfs = Arc::new(ContainerRootfsSnapshotProvider::new(
            container_id.clone(),
            generation,
            Arc::new(DockerContainerExportSource::new(Arc::clone(&self.client))),
            root_archive.clone(),
            TarLimits::default(),
            snapshot_mounts,
            SNAPSHOT_TIMEOUT,
        )?);
        let router = Arc::new(ContainerProviderRouter::new(
            container_id.clone(),
            rootfs.clone(),
            mounts,
        )?);
        let path = container_namespace_path(&container_id)?;
        let router_dyn: Arc<dyn ReadOnlyFilesystemProvider> = router.clone();
        plan.mounts.push(NamespaceMount {
            path: path.clone(),
            key: format!("container:{container_id}"),
            provider: router_dyn,
        });
        let friendly_name = detail.summary.name.trim_start_matches('/');
        if !friendly_name.is_empty() {
            plan.aliases.push(NamespaceAlias {
                path: encoded_path("/containers", friendly_name)?,
                target: VirtualPathBytes::new(path.as_bytes())?,
            });
        }
        plan.registries.archives.push(root_archive);
        plan.registries.rootfs.insert(container_id.clone(), rootfs);
        plan.registries.containers.insert(container_id, router);
        Ok(())
    }

    async fn container_mounts(
        &self,
        plan: &mut NamespacePlan,
        detail: &ContainerDetail,
        archive_source: Arc<DockerContainerArchiveSource>,
    ) -> (Vec<ResolvedContainerMount>, Vec<SnapshotMount>) {
        let container_id = &detail.summary.id;
        let mut mounts = Vec::new();
        let mut destinations = HashSet::new();
        for mount in &detail.mounts {
            if let Ok(destination) = VirtualPath::from_absolute(mount.destination.as_bytes()) {
                if !destination.is_root() {
                    // Suppress the exported lower layer even when constructing
                    // this particular live mount provider fails.
                    destinations.insert(destination);
                }
            }
            let result = self
                .container_mount(plan, container_id, mount, archive_source.clone())
                .await;
            match result {
                Ok(resolved) => mounts.push(resolved),
                Err(error) => tracing::warn!(
                    %error,
                    container = %container_id,
                    destination = %mount.destination,
                    "container mount VFS provider was not attached"
                ),
            }
        }
        for destination in RUNTIME_MOUNTS {
            let Ok(destination) = VirtualPath::from_absolute(destination.as_bytes()) else {
                continue;
            };
            if destinations.contains(&destination) {
                continue;
            }
            match archive_mount(
                plan,
                container_id,
                destination.clone(),
                archive_source.clone(),
                self.spool.clone(),
                ProviderKind::RuntimeMount,
            ) {
                Ok(mount) => {
                    destinations.insert(destination);
                    mounts.push(mount);
                }
                Err(error) => tracing::warn!(
                    %error,
                    container = %container_id,
                    "container runtime mount VFS provider was not attached"
                ),
            }
        }
        let snapshot_mounts = destinations
            .into_iter()
            .filter_map(|destination| SnapshotMount::new(destination).ok())
            .collect();
        (mounts, snapshot_mounts)
    }

    async fn container_mount(
        &self,
        plan: &mut NamespacePlan,
        container_id: &str,
        mount: &tuxstack_domain::MountInfo,
        archive_source: Arc<DockerContainerArchiveSource>,
    ) -> Result<ResolvedContainerMount> {
        let destination = VirtualPath::from_absolute(mount.destination.as_bytes())?;
        if destination.is_root() {
            bail!("container root mount is unsupported");
        }
        match mount.typed_mount_type() {
            ContainerMountType::Volume => {
                let volume_name = mount
                    .name
                    .as_deref()
                    .or(mount.source.as_deref())
                    .context("named volume mount has no volume name")?;
                let provider = self.volume_pool.provider(volume_name.to_owned());
                plan.registries.volume_names.insert(volume_name.to_owned());
                let provider_dyn: Arc<dyn ReadOnlyFilesystemProvider> = provider;
                Ok(ResolvedContainerMount {
                    destination,
                    provider_key: ProviderKey(format!("volume:{volume_name}")),
                    provider_root: VirtualPath::root(),
                    provider: provider_dyn,
                })
            }
            ContainerMountType::Bind => {
                let source = mount
                    .source
                    .as_deref()
                    .context("bind mount has no source")?;
                match checked_local_bind(source).await {
                    Ok(local) => {
                        let local = Arc::new(local);
                        let provider: Arc<dyn ReadOnlyFilesystemProvider> = local.clone();
                        plan.registries.local_binds.push(local);
                        Ok(ResolvedContainerMount {
                            destination,
                            provider_key: ProviderKey(format!("bind:{source}")),
                            provider_root: VirtualPath::root(),
                            provider,
                        })
                    }
                    Err(error) if helper_bind_fallback_allowed(&error) => {
                        let helper = Arc::new(HelperBindProvider::new(
                            Arc::clone(&self.client),
                            self.daemon_instance.clone(),
                            source,
                        )?);
                        let provider: Arc<dyn ReadOnlyFilesystemProvider> = helper.clone();
                        plan.registries.helper_binds.push(helper);
                        Ok(ResolvedContainerMount {
                            destination,
                            provider_key: ProviderKey(format!("helper-bind:{source}")),
                            provider_root: VirtualPath::root(),
                            provider,
                        })
                    }
                    Err(error) => Err(error.into()),
                }
            }
            ContainerMountType::Tmpfs => archive_mount(
                plan,
                container_id,
                destination,
                archive_source,
                self.spool.clone(),
                ProviderKind::TmpfsLive,
            ),
            ContainerMountType::NamedPipe
            | ContainerMountType::Cluster
            | ContainerMountType::Image
            | ContainerMountType::Unknown => archive_mount(
                plan,
                container_id,
                destination,
                archive_source,
                self.spool.clone(),
                ProviderKind::RuntimeMount,
            ),
        }
    }

    fn start_events(self: &Arc<Self>) {
        let monitor = self.event_monitor.clone();
        let stream = monitor.start();
        tokio::spawn(async move {
            run_monitor(&monitor, stream, DefaultEventClassifier).await;
        });
        let mut changes = self.event_monitor.subscribe();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let Some(state) = weak.upgrade() else { break };
                let Some(change) = changes.borrow_and_update().clone() else {
                    continue;
                };
                state.handle_change(change).await;
            }
        });
    }

    async fn handle_change(&self, notification: ChangeNotification) {
        let rebuild = notification.kinds.iter().any(|kind| {
            matches!(
                kind,
                ChangeKind::Containers | ChangeKind::Images | ChangeKind::Volumes
            )
        });
        if rebuild {
            if let Err(error) = self.rebuild_namespace().await {
                tracing::warn!(%error, "failed to rebuild VFS namespace after Docker events");
            }
            if let Some(notifier) = read(&self.notifier).clone() {
                if let Err(error) = notifier.invalidate_inode(ROOT_INODE) {
                    tracing::debug!(%error, "failed to invalidate VFS namespace root");
                }
            }
        }
        self.publish_change(notification);
    }

    fn publish_change(&self, notification: ChangeNotification) {
        for kind in notification.kinds {
            let resource_kind = match kind {
                ChangeKind::Containers => ResourceKind::Container,
                ChangeKind::Images => ResourceKind::Image,
                ChangeKind::Volumes => ResourceKind::Volume,
                ChangeKind::Networks | ChangeKind::Daemon => continue,
            };
            let _ = self.events.send(DaemonEvent::Resource {
                kind: resource_kind,
                resource: None,
                change: ResourceChange::Invalidated,
            });
        }
    }
}

fn add_volume(
    plan: &mut NamespacePlan,
    pool: &NamedVolumeProviderPool,
    volume_name: &str,
) -> Result<()> {
    let provider = pool.provider(volume_name.to_owned());
    let provider_dyn: Arc<dyn ReadOnlyFilesystemProvider> = provider;
    plan.mounts.push(NamespaceMount {
        path: encoded_path("/volumes", volume_name)?,
        key: format!("volume:{volume_name}"),
        provider: provider_dyn,
    });
    plan.registries.volume_names.insert(volume_name.to_owned());
    Ok(())
}

fn archive_mount(
    plan: &mut NamespacePlan,
    container_id: &str,
    destination: VirtualPath,
    source: Arc<DockerContainerArchiveSource>,
    spool: ContentSpool,
    kind: ProviderKind,
) -> Result<ResolvedContainerMount> {
    let provider = Arc::new(ContainerArchiveProvider::with_kind(
        container_id,
        source,
        spool,
        TarLimits::default(),
        OPERATION_TIMEOUT,
        kind,
        Some(String::from_utf8_lossy(&destination.as_bytes()).into_owned()),
    )?);
    let provider_dyn: Arc<dyn ReadOnlyFilesystemProvider> = provider.clone();
    let provider_key = ProviderKey(format!(
        "{}:{}:{}",
        provider_kind_name(kind),
        container_id,
        String::from_utf8_lossy(&destination.as_bytes())
    ));
    plan.registries.archives.push(provider);
    Ok(ResolvedContainerMount {
        provider_root: destination.clone(),
        destination,
        provider_key,
        provider: provider_dyn,
    })
}

fn descriptor_for_route(
    registries: &ProviderRegistries,
    namespace_key: &str,
    relative_path: &VirtualPath,
    namespace_provider: &Arc<dyn ReadOnlyFilesystemProvider>,
) -> Result<ProviderDescriptor> {
    if let Some(container_id) = namespace_key.strip_prefix("container:") {
        let router = registries
            .containers
            .get(container_id)
            .context("container provider registry is out of sync")?;
        return Ok(router
            .router()
            .route(&tuxstack_vfs::ContainerPath(relative_path.clone()))?
            .provider
            .descriptor());
    }
    Ok(namespace_provider.descriptor())
}

fn wire_descriptor(descriptor: ProviderDescriptor) -> WireDescriptor {
    let consistency = match descriptor.consistency {
        ConsistencyMode::Immutable => WireConsistency::Immutable,
        ConsistencyMode::Live => WireConsistency::Live,
        ConsistencyMode::Snapshot {
            captured_at,
            generation,
        } => WireConsistency::Snapshot {
            captured_at_unix_ms: captured_at.timestamp_millis(),
            generation,
        },
        ConsistencyMode::OperationTimeRead => WireConsistency::OperationTimeRead,
        ConsistencyMode::Unavailable => WireConsistency::Unavailable,
    };
    let status = if consistency == WireConsistency::Unavailable {
        ProviderStatus::Unavailable {
            reason: "snapshot has not been captured".to_owned(),
        }
    } else {
        ProviderStatus::Ready
    };
    WireDescriptor {
        kind: wire_provider_kind(descriptor.kind),
        consistency,
        source: descriptor.source,
        capabilities: WireCapabilities(descriptor.capabilities.bits()),
        status,
    }
}

fn wire_provider_kind(kind: ProviderKind) -> WireProviderKind {
    match kind {
        ProviderKind::ContainerRootfsSnapshot => WireProviderKind::ContainerRootfsSnapshot,
        ProviderKind::ContainerArchiveLive => WireProviderKind::ContainerArchiveLive,
        ProviderKind::NamedVolumeLive => WireProviderKind::NamedVolumeLive,
        ProviderKind::LocalBindLive => WireProviderKind::LocalBindLive,
        ProviderKind::HelperBindLive => WireProviderKind::HelperBindLive,
        ProviderKind::TmpfsLive => WireProviderKind::TmpfsLive,
        ProviderKind::RuntimeMount => WireProviderKind::RuntimeMount,
        ProviderKind::ImageRootfsImmutable => WireProviderKind::ImageRootfsImmutable,
        ProviderKind::InMemory => unreachable!("synthetic namespace is not a resource provider"),
    }
}

fn provider_kind_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::TmpfsLive => "tmpfs",
        ProviderKind::RuntimeMount => "runtime",
        _ => "archive",
    }
}

async fn checked_local_bind(source: &str) -> Result<LocalBindProvider, VfsError> {
    let provider = LocalBindProvider::new(source).await?;
    let context = RequestContext {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        pid: std::process::id(),
        request_id: 0,
    };
    // Probe actual directory access, not merely construction of the retained
    // root FD, before deciding that helper fallback is unnecessary.
    provider
        .read_dir(&VirtualPath::root(), &context)
        .await
        .map(drop)?;
    Ok(provider)
}

fn helper_bind_fallback_allowed(error: &VfsError) -> bool {
    match error {
        VfsError::PermissionDenied => true,
        VfsError::Unavailable(reason) | VfsError::Io(reason) => {
            let reason = reason.to_ascii_lowercase();
            reason.contains("openat2")
                || reason.contains("not supported")
                || reason.contains("unsupported")
                || reason.contains("linux filesystem error 38")
                || reason.contains("linux filesystem error 95")
        }
        _ => false,
    }
}

fn resource_namespace_path(resource: &DockerResourceRef) -> Result<VirtualPath> {
    match resource {
        DockerResourceRef::Container { container_id } => container_namespace_path(container_id),
        DockerResourceRef::Image { image_id } => image_namespace_path(image_id),
        DockerResourceRef::Volume { volume_name } => encoded_path("/volumes", volume_name),
        _ => bail!("unsupported resource kind"),
    }
}

fn container_namespace_path(container_id: &str) -> Result<VirtualPath> {
    encoded_path("/containers/.by-id", container_id)
}

fn image_namespace_path(image_id: &str) -> Result<VirtualPath> {
    encoded_path("/images/.by-id", image_id)
}

fn encoded_path(parent: &str, value: &str) -> Result<VirtualPath> {
    let parent = VirtualPath::from_absolute(parent.as_bytes())?;
    let encoded = FuseNameCodec::encode(value.as_bytes())?;
    Ok(parent.join(&VirtualFileName::new(encoded.as_bytes())?)?)
}

fn host_namespace_path(mount_point: &Path, path: &VirtualPath) -> PathBuf {
    path.components()
        .iter()
        .fold(mount_point.to_owned(), |path, component| {
            path.join(component.as_os_str())
        })
}

async fn shutdown_helper_binds(providers: Vec<Arc<HelperBindProvider>>) {
    for provider in providers {
        if let Err(error) = provider.shutdown().await {
            tracing::warn!(%error, "failed to stop helper-backed bind provider");
        }
    }
}

pub fn recover_stale_mount(mount_point: &Path) -> Result<()> {
    let mounted = std::process::Command::new("mountpoint")
        .arg("-q")
        .arg(mount_point)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !mounted {
        return Ok(());
    }
    let status = std::process::Command::new("fusermount3")
        .arg("-u")
        .arg("-z")
        .arg(mount_point)
        .status()
        .context("run fusermount3 for stale TuxStack mount")?;
    if !status.success() {
        bail!("failed to recover stale mount {}", mount_point.display());
    }
    Ok(())
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn lock<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_snapshot_is_never_reported_ready() {
        let descriptor = wire_descriptor(ProviderDescriptor {
            kind: ProviderKind::ContainerRootfsSnapshot,
            consistency: ConsistencyMode::Unavailable,
            source: Some("container".into()),
            capabilities: tuxstack_vfs::ProviderCapabilities::READ_ONLY,
        });
        assert_eq!(descriptor.consistency, WireConsistency::Unavailable);
        assert!(matches!(
            descriptor.status,
            ProviderStatus::Unavailable { .. }
        ));
    }

    #[test]
    fn namespace_paths_encode_resource_names_as_one_component() {
        let path = encoded_path("/images", "registry:5000/team/image:tag").unwrap();
        assert_eq!(path.depth(), 2);
        assert_eq!(
            FuseNameCodec::decode(
                std::str::from_utf8(path.file_name().unwrap().as_bytes()).unwrap()
            )
            .unwrap(),
            b"registry:5000/team/image:tag"
        );
    }

    #[test]
    fn helper_fallback_is_restricted_to_permission_and_unsupported_access() {
        assert!(helper_bind_fallback_allowed(&VfsError::PermissionDenied));
        assert!(helper_bind_fallback_allowed(&VfsError::Unavailable(
            "Linux openat2 is required".into()
        )));
        assert!(!helper_bind_fallback_allowed(&VfsError::NotFound));
        assert!(!helper_bind_fallback_allowed(&VfsError::InvalidInput(
            "bad bind"
        )));
    }
}
