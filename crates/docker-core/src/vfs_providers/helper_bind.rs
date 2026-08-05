//! Hardened helper-container fallback for bind sources which the daemon user
//! cannot access directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, ResourcesUlimits};
use bollard::query_parameters::{CreateContainerOptions, ListContainersOptions};
use bytes::Bytes;
use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tuxstack_vfs::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualMetadata, VirtualPath, VirtualPathBytes,
};
use uuid::Uuid;

use crate::{
    DockerClient, FilesystemError, FilesystemService, FilesystemSession, FilesystemSource,
};

use super::support::{HelperProviderCore, SessionFactory};

const HELPER_PATH: &str = "/usr/bin/tuxstack-fs-helper";
const SOURCE_TARGET: &str = "/source";
const LABEL_MANAGED: &str = "io.github.tuxstack.managed";
const LABEL_PURPOSE: &str = "io.github.tuxstack.purpose";
const LABEL_DAEMON_INSTANCE: &str = "io.github.tuxstack.daemon-instance";
const LABEL_SESSION: &str = "io.github.tuxstack.session";
const PURPOSE_BIND_FILES: &str = "bind-files";
const DEFAULT_DIRECTORY_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct BindSessionFactory {
    client: Arc<DockerClient>,
    source_root: Arc<PathBuf>,
    daemon_instance: Arc<str>,
    timeout: Duration,
}

#[async_trait]
impl SessionFactory for BindSessionFactory {
    async fn start(
        &self,
        cancellation: CancellationToken,
    ) -> Result<FilesystemSession, FilesystemError> {
        create_bind_session(
            &self.client,
            &self.source_root,
            &self.daemon_instance,
            self.timeout,
            &cancellation,
        )
        .await
    }
}

/// Helper-backed live bind provider. Construct this only after direct
/// `LocalBindProvider` creation/access fails with a permission or unsupported
/// mount error.
pub struct HelperBindProvider {
    source_root: Arc<PathBuf>,
    daemon_instance: Arc<str>,
    core: HelperProviderCore,
}

impl HelperBindProvider {
    /// `client` must be the daemon's existing DockerClient. This constructor
    /// wraps it in the existing FilesystemService and does not connect to or
    /// create a second Docker backend.
    pub fn new(
        client: Arc<DockerClient>,
        daemon_instance: impl Into<Arc<str>>,
        source_root: impl Into<PathBuf>,
    ) -> Result<Self, VfsError> {
        Self::with_options(
            client,
            daemon_instance,
            source_root,
            DEFAULT_DIRECTORY_TTL,
            Duration::from_secs(60),
        )
    }

    pub fn with_options(
        client: Arc<DockerClient>,
        daemon_instance: impl Into<Arc<str>>,
        source_root: impl Into<PathBuf>,
        directory_ttl: Duration,
        operation_timeout: Duration,
    ) -> Result<Self, VfsError> {
        if !client.is_local() {
            return Err(VfsError::Unavailable(
                "helper bind requires the daemon's local Docker Engine".into(),
            ));
        }
        if !(Duration::from_secs(1)..=Duration::from_secs(3)).contains(&directory_ttl) {
            return Err(VfsError::InvalidInput(
                "helper-bind directory TTL must be between 1 and 3 seconds",
            ));
        }
        let source_root = source_root.into();
        validate_source_root(&source_root)?;
        if source_root.to_str().is_none() {
            return Err(VfsError::InvalidInput(
                "helper bind source must be valid UTF-8 for the Docker API",
            ));
        }
        if contains_docker_socket(&source_root, client.socket_path().map(PathBuf::as_path)) {
            return Err(VfsError::PermissionDenied);
        }
        let daemon_instance = daemon_instance.into();
        if daemon_instance.is_empty() {
            return Err(VfsError::InvalidInput(
                "daemon instance label must not be empty",
            ));
        }

        let source_root = Arc::new(source_root);
        let service = Arc::new(FilesystemService::with_timeout(
            Arc::clone(&client),
            operation_timeout,
        ));
        let factory = Arc::new(BindSessionFactory {
            client,
            source_root: Arc::clone(&source_root),
            daemon_instance: Arc::clone(&daemon_instance),
            timeout: operation_timeout,
        });
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::HelperBindLive,
            consistency: ConsistencyMode::Live,
            source: Some(source_root.to_string_lossy().into_owned()),
            capabilities: ProviderCapabilities::READ_ONLY,
        };
        let mut namespace = b"helper-bind\0".to_vec();
        namespace.extend_from_slice(daemon_instance.as_bytes());
        namespace.push(0);
        namespace.extend_from_slice(source_root.as_os_str().as_encoded_bytes());

        Ok(Self {
            source_root,
            daemon_instance,
            core: HelperProviderCore::new(descriptor, namespace, service, factory, directory_ttl),
        })
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn daemon_instance(&self) -> &str {
        &self.daemon_instance
    }

    pub async fn shutdown(&self) -> Result<(), VfsError> {
        self.core.shutdown().await
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for HelperBindProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.core.descriptor()
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.core.lookup(parent, name).await
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.core.getattr(path).await
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        self.core.read_dir(path).await
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        self.core.read_link(path).await
    }

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        _ctx: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        self.core.open(path, flags).await
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        _ctx: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        self.core.read_at(handle, offset, size).await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        self.core.close(handle).await
    }

    async fn refresh(&self, path: Option<&VirtualPath>) -> Result<(), VfsError> {
        self.core.refresh(path).await;
        Ok(())
    }
}

async fn create_bind_session(
    client: &DockerClient,
    source_root: &Path,
    daemon_instance: &str,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<FilesystemSession, FilesystemError> {
    if cancellation.is_cancelled() {
        return Err(FilesystemError::Cancelled);
    }
    let docker = client.inner();
    crate::services::filesystem::volume_provider::ensure_helper_image(docker, timeout).await?;

    let session_id = Uuid::new_v4().to_string();
    let container_name = format!("tuxstack-bind-helper-{session_id}");
    let labels = helper_labels(daemon_instance, &session_id);
    let host_config = hardened_host_config(source_root);
    let response = tokio::time::timeout(
        timeout,
        docker.create_container(
            Some(CreateContainerOptions {
                name: Some(container_name.clone()),
                platform: String::new(),
            }),
            ContainerCreateBody {
                image: Some(helper_image_tag()),
                entrypoint: Some(vec![HELPER_PATH.into()]),
                cmd: Some(vec!["hold".into()]),
                user: Some("0".into()),
                labels: Some(labels),
                host_config: Some(host_config),
                ..Default::default()
            },
        ),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(|error| FilesystemError::HelperContainerCreateFailed(error.to_string()))?;

    let provisional = FilesystemSession {
        container_id: response.id,
        container_name,
        // The legacy enum has no Bind variant. This field is diagnostic only;
        // the helper root and exact labels define the bind session.
        source: FilesystemSource::Volume {
            volume_name: format!("bind:{}", source_root.display()),
        },
        root: SOURCE_TARGET.into(),
        helper_path: HELPER_PATH.into(),
        protocol_version: tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION,
        helper_version: String::new(),
        read_only: true,
        created_at: Utc::now(),
    };

    let start = tokio::time::timeout(
        timeout,
        docker.start_container(&provisional.container_id, None),
    )
    .await;
    match start {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = crate::services::filesystem::session::invalidate_session(docker, &provisional)
                .await;
            return Err(FilesystemError::HelperContainerStartFailed(
                error.to_string(),
            ));
        }
        Err(_) => {
            let _ = crate::services::filesystem::session::invalidate_session(docker, &provisional)
                .await;
            return Err(FilesystemError::Timeout);
        }
    }

    let helper_version = match crate::services::filesystem::client::hello(
        docker,
        &provisional,
        timeout,
        cancellation,
    )
    .await
    {
        Ok(version) => version,
        Err(error) => {
            let _ = crate::services::filesystem::session::invalidate_session(docker, &provisional)
                .await;
            return Err(error);
        }
    };
    Ok(FilesystemSession {
        helper_version,
        ..provisional
    })
}

/// Remove orphaned bind helpers for one exact daemon instance. Listing and
/// deletion both use the complete managed/purpose/instance label tuple; a
/// helper from another daemon instance can never match.
pub async fn cleanup_orphan_bind_helpers(
    client: &DockerClient,
    daemon_instance: &str,
) -> Result<usize, VfsError> {
    if daemon_instance.is_empty() {
        return Err(VfsError::InvalidInput(
            "daemon instance label must not be empty",
        ));
    }
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_owned(),
        vec![
            format!("{LABEL_MANAGED}=true"),
            format!("{LABEL_PURPOSE}={PURPOSE_BIND_FILES}"),
            format!("{LABEL_DAEMON_INSTANCE}={daemon_instance}"),
        ],
    );
    let docker = client.inner();
    let containers = tokio::time::timeout(
        Duration::from_secs(15),
        docker.list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(filters),
            ..Default::default()
        })),
    )
    .await
    .map_err(|_| VfsError::TimedOut)?
    .map_err(|error| VfsError::Unavailable(error.to_string()))?;

    let mut removed = 0;
    for container in containers {
        let labels = container.labels.unwrap_or_default();
        if labels.get(LABEL_MANAGED).map(String::as_str) != Some("true")
            || labels.get(LABEL_PURPOSE).map(String::as_str) != Some(PURPOSE_BIND_FILES)
            || labels.get(LABEL_DAEMON_INSTANCE).map(String::as_str) != Some(daemon_instance)
            || !labels.contains_key(LABEL_SESSION)
        {
            continue;
        }
        let Some(id) = container.id else { continue };
        crate::services::filesystem::session::force_remove_container(docker, &id)
            .await
            .map_err(super::support::map_filesystem_error)?;
        removed += 1;
    }
    Ok(removed)
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

fn contains_docker_socket(source_root: &Path, configured_socket: Option<&Path>) -> bool {
    configured_socket.is_some_and(|socket| socket.starts_with(source_root))
        || Path::new("/var/run/docker.sock").starts_with(source_root)
        || Path::new("/run/docker.sock").starts_with(source_root)
}

fn helper_labels(daemon_instance: &str, session_id: &str) -> HashMap<String, String> {
    [
        (LABEL_MANAGED.to_owned(), "true".to_owned()),
        (LABEL_PURPOSE.to_owned(), PURPOSE_BIND_FILES.to_owned()),
        (LABEL_DAEMON_INSTANCE.to_owned(), daemon_instance.to_owned()),
        (LABEL_SESSION.to_owned(), session_id.to_owned()),
    ]
    .into_iter()
    .collect()
}

fn hardened_host_config(source_root: &Path) -> HostConfig {
    HostConfig {
        network_mode: Some("none".into()),
        privileged: Some(false),
        readonly_rootfs: Some(true),
        security_opt: Some(vec!["no-new-privileges:true".into()]),
        cap_drop: Some(vec!["ALL".into()]),
        auto_remove: Some(false),
        memory: Some(128 * 1024 * 1024),
        nano_cpus: Some(250_000_000),
        pids_limit: Some(32),
        mounts: Some(vec![Mount {
            typ: Some(MountType::BIND),
            source: Some(source_root.to_string_lossy().into_owned()),
            target: Some(SOURCE_TARGET.into()),
            read_only: Some(true),
            ..Default::default()
        }]),
        ulimits: Some(vec![ResourcesUlimits {
            name: Some("nofile".into()),
            soft: Some(1024),
            hard: Some(1024),
        }]),
        // No binds other than the single Mount above, no Docker socket,
        // devices, namespaces, or added capabilities are configured.
        binds: None,
        cap_add: None,
        devices: None,
        ..Default::default()
    }
}

fn helper_image_tag() -> String {
    format!("tuxstack.internal/fs-helper:1-{}", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_socket_cannot_be_in_the_helper_mount() {
        assert!(contains_docker_socket(Path::new("/"), None));
        assert!(contains_docker_socket(Path::new("/var/run"), None));
        assert!(contains_docker_socket(
            Path::new("/custom"),
            Some(Path::new("/custom/docker.sock"))
        ));
        assert!(!contains_docker_socket(Path::new("/srv/data"), None));
    }

    #[test]
    fn helper_labels_are_exact_and_complete() {
        let labels = helper_labels("daemon-a", "session-a");
        assert_eq!(labels.len(), 4);
        assert_eq!(labels.get(LABEL_MANAGED).map(String::as_str), Some("true"));
        assert_eq!(
            labels.get(LABEL_PURPOSE).map(String::as_str),
            Some(PURPOSE_BIND_FILES)
        );
        assert_eq!(
            labels.get(LABEL_DAEMON_INSTANCE).map(String::as_str),
            Some("daemon-a")
        );
        assert_eq!(
            labels.get(LABEL_SESSION).map(String::as_str),
            Some("session-a")
        );
    }

    #[test]
    fn helper_configuration_is_hardened_and_has_one_read_only_mount() {
        let config = hardened_host_config(Path::new("/srv/data"));
        assert_eq!(config.network_mode.as_deref(), Some("none"));
        assert_eq!(config.privileged, Some(false));
        assert_eq!(config.readonly_rootfs, Some(true));
        assert_eq!(
            config.cap_drop.as_deref(),
            Some(["ALL".to_owned()].as_slice())
        );
        assert!(
            config
                .security_opt
                .as_ref()
                .unwrap()
                .iter()
                .any(|option| option == "no-new-privileges:true")
        );
        assert!(config.binds.is_none());
        assert!(config.devices.is_none());
        let mounts = config.mounts.unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].typ, Some(MountType::BIND));
        assert_eq!(mounts[0].source.as_deref(), Some("/srv/data"));
        assert_eq!(mounts[0].target.as_deref(), Some(SOURCE_TARGET));
        assert_eq!(mounts[0].read_only, Some(true));
        assert_ne!(mounts[0].source.as_deref(), Some("/var/run/docker.sock"));
    }

    #[test]
    fn helper_descriptor_contract_is_live_and_read_only() {
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::HelperBindLive,
            consistency: ConsistencyMode::Live,
            source: Some("/srv/data".into()),
            capabilities: ProviderCapabilities::READ_ONLY,
        };
        assert_eq!(descriptor.kind, ProviderKind::HelperBindLive);
        assert_eq!(descriptor.consistency, ConsistencyMode::Live);
        assert!(
            !descriptor
                .capabilities
                .contains(ProviderCapabilities::DOWNLOAD)
        );
    }
}
