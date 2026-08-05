use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fuser::BackgroundSession;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::cache::{
    ChangeKind, ChangeNotification, DefaultEventClassifier, DockerEventMonitor, run_monitor,
};
use tuxstack_docker_core::services::{
    DockerServices, ListContainersOptions, ListImagesOptions, ListVolumesOptions,
};
use tuxstack_docker_core::{DockerClient, DockerConfig};
use tuxstack_protocol::{
    ConsistencyMode as WireConsistency, DaemonLifecycle, DaemonStatus, DockerConnectionStatus,
    DockerResourceRef, MountState, MountStatus, ProviderCapabilities as WireCapabilities,
    ProviderDescriptor as WireDescriptor, ProviderKind as WireProviderKind, ProviderStatus,
    ResourceChange, ResourceFusePath, ResourceKind, ResourcePath,
};
use tuxstack_vfs::{
    DockerFilesystemResource, FuseNameCodec, InMemoryProvider, ReadOnlyFilesystemProvider,
    ReadOnlyFuseAdapter, VirtualPath,
};

use crate::config::DaemonPaths;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const HANDLE_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PROVIDER_OPERATIONS: usize = 16;
const MAX_OPEN_HANDLES: usize = 4096;

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

pub struct DaemonState {
    pub paths: DaemonPaths,
    pub services: Arc<DockerServices>,
    pub client: Arc<DockerClient>,
    started_at: Instant,
    lifecycle: RwLock<DaemonLifecycle>,
    daemon_id: Option<String>,
    mount: Mutex<Option<BackgroundSession>>,
    mount_status: RwLock<MountStatus>,
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
        let daemon_id = Some(client.endpoint_fingerprint());
        let services = Arc::new(DockerServices::new(Arc::clone(&client)));
        if let Err(error) = services.filesystem.cleanup_orphan_sessions().await {
            tracing::warn!(%error, "failed to clean orphan filesystem helpers at startup");
        }
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
            mount: Mutex::new(None),
            mount_status: RwLock::new(MountStatus {
                state: MountState::Unmounted,
                mount_point: None,
                read_only: true,
            }),
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
        let provider = self.build_namespace().await?;
        let provider: Arc<dyn ReadOnlyFilesystemProvider> = provider;
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
        let session = adapter
            .spawn_mount(&self.paths.mount_point)
            .with_context(|| format!("mount {}", self.paths.mount_point.display()))?;
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
        let (path, descriptor) = match &resource {
            DockerResourceRef::Container { container_id } => (
                self.paths
                    .mount_point
                    .join("containers/.by-id")
                    .join(FuseNameCodec::encode(container_id.as_bytes())?),
                unavailable_descriptor(
                    WireProviderKind::ContainerRootfsSnapshot,
                    "snapshot not built",
                ),
            ),
            DockerResourceRef::Image { image_id } => (
                self.paths
                    .mount_point
                    .join("images/.by-id")
                    .join(FuseNameCodec::encode(image_id.as_bytes())?),
                unavailable_descriptor(WireProviderKind::ImageRootfsImmutable, "index not built"),
            ),
            DockerResourceRef::Volume { volume_name } => (
                self.paths
                    .mount_point
                    .join("volumes")
                    .join(FuseNameCodec::encode(volume_name.as_bytes())?),
                unavailable_descriptor(
                    WireProviderKind::NamedVolumeLive,
                    "live provider is not attached",
                ),
            ),
            _ => bail!("unsupported resource kind"),
        };
        Ok(ResourceFusePath {
            resource,
            path,
            descriptor,
        })
    }

    pub async fn provider_descriptor(&self, path: ResourcePath) -> Result<WireDescriptor> {
        Ok(self.resource_path(path.resource).await?.descriptor)
    }

    pub async fn stop(&self) -> Result<()> {
        *write(&self.lifecycle) = DaemonLifecycle::Stopping;
        let _ = self.events.send(DaemonEvent::Status(self.status()));
        self.shutdown.cancel();
        self.event_monitor.shutdown();
        let _ = self.unmount().await;
        if let Err(error) = self.services.filesystem.cleanup_orphan_sessions().await {
            tracing::warn!(%error, "failed to clean filesystem helpers during shutdown");
        }
        let _ = std::fs::remove_file(&self.paths.socket_path);
        Ok(())
    }

    async fn build_namespace(&self) -> Result<Arc<InMemoryProvider>> {
        let provider = Arc::new(InMemoryProvider::new());
        add_dir(&provider, b"/containers")?;
        add_dir(&provider, b"/containers/.by-id")?;
        add_dir(&provider, b"/images")?;
        add_dir(&provider, b"/images/.by-id")?;
        add_dir(&provider, b"/volumes")?;

        let container_options = ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let image_options = ListImagesOptions::default();
        let volume_options = ListVolumesOptions::default();
        let (containers, images, volumes) = tokio::try_join!(
            self.services.containers.list_containers(&container_options),
            self.services.images.list_images(&image_options),
            self.services.volumes.list_volumes(&volume_options),
        )?;
        for container in containers {
            add_dir(
                &provider,
                format!(
                    "/containers/.by-id/{}",
                    FuseNameCodec::encode(container.id.as_bytes())?
                )
                .as_bytes(),
            )?;
        }
        for image in images {
            add_dir(
                &provider,
                format!(
                    "/images/.by-id/{}",
                    FuseNameCodec::encode(image.id.as_bytes())?
                )
                .as_bytes(),
            )?;
        }
        for volume in volumes {
            add_dir(
                &provider,
                format!(
                    "/volumes/{}",
                    FuseNameCodec::encode(volume.name.as_bytes())?
                )
                .as_bytes(),
            )?;
        }
        Ok(provider)
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
                state.publish_change(change);
            }
        });
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

fn add_dir(provider: &InMemoryProvider, absolute: &[u8]) -> Result<()> {
    let path = VirtualPath::from_absolute(absolute)?;
    let node_id = absolute.to_vec();
    provider.add_directory(path, node_id)?;
    Ok(())
}

fn unavailable_descriptor(kind: WireProviderKind, reason: &str) -> WireDescriptor {
    WireDescriptor {
        kind,
        consistency: WireConsistency::Unavailable,
        source: None,
        capabilities: WireCapabilities::NONE,
        status: ProviderStatus::Unavailable {
            reason: reason.to_owned(),
        },
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
    fn unavailable_descriptor_never_claims_live() {
        let descriptor = unavailable_descriptor(WireProviderKind::ContainerRootfsSnapshot, "x");
        assert_eq!(descriptor.consistency, WireConsistency::Unavailable);
        assert!(matches!(
            descriptor.status,
            ProviderStatus::Unavailable { .. }
        ));
    }
}
