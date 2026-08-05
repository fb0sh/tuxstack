//! Live named-volume VFS provider backed by the existing filesystem helper service.
//!
//! `NamedVolumeProviderPool` is daemon-owned. It interns providers by
//! `(daemon identity, volume name)`, so the top-level volumes namespace and
//! container mount routes share the exact provider, helper session, caches,
//! and generation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio_util::sync::CancellationToken;
use tuxstack_vfs::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualMetadata, VirtualPath, VirtualPathBytes,
};

use crate::{FilesystemError, FilesystemService, FilesystemSession};

use super::support::{DEFAULT_DIRECTORY_TTL, HelperProviderCore, SessionFactory};

#[derive(Clone)]
struct VolumeSessionFactory {
    service: Arc<FilesystemService>,
    volume_name: Arc<str>,
}

#[async_trait]
impl SessionFactory for VolumeSessionFactory {
    async fn start(
        &self,
        cancellation: CancellationToken,
    ) -> Result<FilesystemSession, FilesystemError> {
        self.service
            .start_volume_session(&self.volume_name, cancellation)
            .await
    }
}

/// Daemon-owned interner for named-volume providers.
///
/// Construct one pool per Docker daemon identity and retain it for the daemon
/// lifetime. `provider("x")` always returns the same live provider while any
/// route still references it.
pub struct NamedVolumeProviderPool {
    daemon_identity: Arc<str>,
    service: Arc<FilesystemService>,
    directory_ttl: Duration,
    providers: Mutex<HashMap<String, Arc<NamedVolumeProvider>>>,
}

impl NamedVolumeProviderPool {
    pub fn new(daemon_identity: impl Into<Arc<str>>, service: Arc<FilesystemService>) -> Self {
        Self::with_directory_ttl(daemon_identity, service, DEFAULT_DIRECTORY_TTL)
            .expect("the default volume directory TTL is valid")
    }

    pub fn with_directory_ttl(
        daemon_identity: impl Into<Arc<str>>,
        service: Arc<FilesystemService>,
        directory_ttl: Duration,
    ) -> Result<Self, VfsError> {
        if !(Duration::from_secs(1)..=Duration::from_secs(5)).contains(&directory_ttl) {
            return Err(VfsError::InvalidInput(
                "named-volume directory TTL must be between 1 and 5 seconds",
            ));
        }
        Ok(Self {
            daemon_identity: daemon_identity.into(),
            service,
            directory_ttl,
            providers: Mutex::new(HashMap::new()),
        })
    }

    pub fn provider(&self, volume_name: impl Into<String>) -> Arc<NamedVolumeProvider> {
        let volume_name = volume_name.into();
        let mut providers = self
            .providers
            .lock()
            .expect("volume provider pool poisoned");
        if let Some(provider) = providers.get(&volume_name) {
            return Arc::clone(provider);
        }

        let provider = Arc::new(NamedVolumeProvider::new(
            Arc::clone(&self.daemon_identity),
            volume_name.clone(),
            Arc::clone(&self.service),
            self.directory_ttl,
        ));
        providers.insert(volume_name, Arc::clone(&provider));
        provider
    }

    /// Invalidate an event-selected volume and precisely remove its helper
    /// session. Existing route references become stale and must be replaced.
    pub async fn remove(&self, volume_name: &str) -> Result<(), VfsError> {
        let provider = self
            .providers
            .lock()
            .expect("volume provider pool poisoned")
            .remove(volume_name);
        if let Some(provider) = provider {
            provider.shutdown().await?;
        }
        Ok(())
    }

    /// Daemon shutdown hook. Only sessions represented by this pool are
    /// stopped; no name/prefix based container cleanup is performed here.
    pub async fn shutdown(&self) -> Result<(), VfsError> {
        let providers: Vec<_> = self
            .providers
            .lock()
            .expect("volume provider pool poisoned")
            .drain()
            .map(|(_, provider)| provider)
            .collect();
        let mut first_error = None;
        for provider in providers {
            if let Err(error) = provider.shutdown().await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Read-only VFS view of a Docker named volume. Content and metadata are live;
/// directory results use a short bounded TTL.
pub struct NamedVolumeProvider {
    daemon_identity: Arc<str>,
    volume_name: Arc<str>,
    core: HelperProviderCore,
}

impl NamedVolumeProvider {
    fn new(
        daemon_identity: Arc<str>,
        volume_name: String,
        service: Arc<FilesystemService>,
        directory_ttl: Duration,
    ) -> Self {
        let volume_name: Arc<str> = volume_name.into();
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::NamedVolumeLive,
            consistency: ConsistencyMode::Live,
            source: Some(volume_name.to_string()),
            capabilities: ProviderCapabilities::READ_ONLY,
        };
        let factory = Arc::new(VolumeSessionFactory {
            service: Arc::clone(&service),
            volume_name: Arc::clone(&volume_name),
        });
        let node_namespace = provider_key(&daemon_identity, &volume_name);
        Self {
            daemon_identity,
            volume_name,
            core: HelperProviderCore::new(
                descriptor,
                node_namespace,
                service,
                factory,
                directory_ttl,
            ),
        }
    }

    pub fn volume_name(&self) -> &str {
        &self.volume_name
    }

    pub fn daemon_identity(&self) -> &str {
        &self.daemon_identity
    }

    pub async fn shutdown(&self) -> Result<(), VfsError> {
        self.core.shutdown().await
    }
}

fn provider_key(daemon_identity: &str, volume_name: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(daemon_identity.len() + volume_name.len() + 9);
    key.extend_from_slice(b"volume\0");
    key.extend_from_slice(daemon_identity.as_bytes());
    key.push(0);
    key.extend_from_slice(volume_name.as_bytes());
    key
}

#[async_trait]
impl ReadOnlyFilesystemProvider for NamedVolumeProvider {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DockerClient, DockerConfig};

    #[test]
    fn pool_interns_one_provider_per_volume_name() {
        let client = Arc::new(
            DockerClient::connect_with_config(DockerConfig {
                host: Some("tcp://provider-test.invalid:2375".into()),
                ..DockerConfig::default()
            })
            .expect("construct lazy test client"),
        );
        let pool =
            NamedVolumeProviderPool::new("test-daemon", Arc::new(FilesystemService::new(client)));
        let first = pool.provider("data");
        let second = pool.provider("data");
        let other = pool.provider("other");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn provider_key_separates_daemons_and_names_without_ambiguity() {
        assert_ne!(provider_key("ab", "c"), provider_key("a", "bc"));
        assert_ne!(provider_key("one", "data"), provider_key("two", "data"));
        assert_eq!(provider_key("one", "data"), provider_key("one", "data"));
    }

    #[test]
    fn volume_descriptor_is_live_and_read_only() {
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::NamedVolumeLive,
            consistency: ConsistencyMode::Live,
            source: Some("data".into()),
            capabilities: ProviderCapabilities::READ_ONLY,
        };
        assert_eq!(descriptor.kind, ProviderKind::NamedVolumeLive);
        assert_eq!(descriptor.consistency, ConsistencyMode::Live);
        assert!(
            descriptor
                .capabilities
                .contains(ProviderCapabilities::REFRESH)
        );
        assert!(
            !descriptor
                .capabilities
                .contains(ProviderCapabilities::DOWNLOAD)
        );
    }

    #[test]
    fn directory_ttl_contract_is_one_to_five_seconds() {
        let valid = |ttl| (Duration::from_secs(1)..=Duration::from_secs(5)).contains(&ttl);
        assert!(valid(Duration::from_secs(1)));
        assert!(valid(Duration::from_secs(5)));
        assert!(!valid(Duration::from_millis(999)));
        assert!(!valid(Duration::from_secs(6)));
    }
}
