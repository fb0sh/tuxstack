//! Shared application state: the services registry, page status
//! machine, and pure (testable) page-state logic.
//!
//! The QML-facing bridge objects are thin; all state transitions live
//! here so they can be unit-tested without Qt.

use std::sync::{Arc, Mutex, OnceLock};

use tuxstack_docker_core::cache::{
    DockerEventMonitor, EndpointKey, ImageMetadataCache, PersistentCache, PersistentCacheConfig,
    PreviewSessionPool, VolumeUsageCache,
};
use tuxstack_docker_core::{DockerError, DockerServices, FilesystemSession};

use crate::error::AppError;
use crate::settings::GuiSettings;

/// Registry of the shared Docker services, set once the app connects.
static SERVICES: Mutex<Option<Arc<DockerServices>>> = Mutex::new(None);

/// Registry of GUI settings (set at startup).
static SETTINGS: OnceLock<GuiSettings> = OnceLock::new();

/// Shared cache layer: persistent SQLite snapshot plus in-memory caches
/// used to display cached data instantly while Docker refreshes in the
/// background (stale-while-revalidate). Created once per connection;
/// re-pointed to the new endpoint when Docker reconnects.
#[derive(Clone)]
pub struct DockerStore {
    pub persistent: Option<PersistentCache>,
    pub endpoint: Option<EndpointKey>,
    pub image_metadata: ImageMetadataCache,
    pub volume_usage: VolumeUsageCache,
    pub preview_sessions: PreviewSessionPool<FilesystemSession>,
    /// Pool of live image-browsing helper containers keyed by image id.
    pub image_preview_sessions: PreviewSessionPool<FilesystemSession>,
    /// Watches `/events` and publishes debounced change notifications.
    pub events: DockerEventMonitor,
}

impl Default for DockerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerStore {
    pub fn new() -> Self {
        Self {
            persistent: None,
            endpoint: None,
            image_metadata: ImageMetadataCache::new(None, None),
            volume_usage: VolumeUsageCache::new(None, None),
            preview_sessions: PreviewSessionPool::<FilesystemSession>::new(),
            image_preview_sessions: PreviewSessionPool::<FilesystemSession>::new(),
            events: DockerEventMonitor::new(),
        }
    }

    /// Re-point the store to a fresh connection (or clear it on disconnect).
    pub fn rebind(&mut self, services: Option<&DockerServices>) {
        let (persistent, endpoint) = match services {
            Some(services) => {
                let path = tuxstack_docker_core::cache::default_cache_path();
                let persistent = PersistentCache::open(&PersistentCacheConfig {
                    path,
                    flush_debounce: std::time::Duration::from_millis(700),
                })
                .ok();
                (persistent, Some(endpoint_key_for_services(services)))
            }
            None => (None, None),
        };
        self.persistent = persistent.clone();
        self.endpoint = endpoint.clone();
        self.image_metadata = ImageMetadataCache::new(persistent.clone(), endpoint.clone());
        self.volume_usage = VolumeUsageCache::new(persistent, endpoint);
        self.preview_sessions = PreviewSessionPool::<FilesystemSession>::new();
        self.image_preview_sessions = PreviewSessionPool::<FilesystemSession>::new();
        match services {
            Some(services) => {
                // Fresh token: a previous disconnect cancelled the old one.
                self.events = DockerEventMonitor::new();
                self.events.rebind(services.client());
            }
            None => self.events.shutdown(),
        }
    }
}

fn endpoint_key_for_services(services: &DockerServices) -> EndpointKey {
    EndpointKey {
        fingerprint: services.client().endpoint_fingerprint(),
        // Docker exposes the daemon ID only through the asynchronous `/info`
        // API. `rebind` is deliberately synchronous and the connected client
        // does not retain a previously fetched ID, so it is unavailable here.
        // The resolved endpoint fingerprint still isolates different daemons;
        // daemon replacement at the same endpoint remains the known limit.
        daemon_id: None,
        context_name: None,
    }
}

/// Registry of the shared Docker services and cache store.
static STORE: OnceLock<Mutex<DockerStore>> = OnceLock::new();

/// Access the shared cache store.
pub fn get_store() -> DockerStore {
    STORE
        .get_or_init(|| Mutex::new(DockerStore::new()))
        .lock()
        .expect("store lock")
        .clone()
}

/// Store the shared services after a successful connection.
pub fn set_services(services: DockerServices) {
    {
        let mut store = STORE
            .get_or_init(|| Mutex::new(DockerStore::new()))
            .lock()
            .expect("store lock");
        store.rebind(Some(&services));
    }
    *SERVICES.lock().expect("services lock") = Some(Arc::new(services));
}

/// Clear a previous connection before starting a new connection attempt.
pub fn clear_services() {
    {
        let mut store = STORE
            .get_or_init(|| Mutex::new(DockerStore::new()))
            .lock()
            .expect("store lock");
        store.rebind(None);
    }
    *SERVICES.lock().expect("services lock") = None;
}

/// Access the shared services, if connected.
pub fn get_services() -> Option<Arc<DockerServices>> {
    SERVICES.lock().expect("services lock").clone()
}

/// Initialize GUI settings (call once at startup).
pub fn set_settings(settings: GuiSettings) {
    let _ = SETTINGS.set(settings);
}

/// Access GUI settings.
pub fn settings() -> &'static GuiSettings {
    SETTINGS.get().expect("settings must be initialized")
}

/// Generic load state used across pages.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // referenced by docs/architecture as the shared load-state type
pub enum LoadState<T> {
    Idle,
    Loading,
    Ready(T),
    Error(AppError),
}

impl<T> LoadState<T> {
    #[allow(dead_code)]
    pub fn is_loading(&self) -> bool {
        matches!(self, LoadState::Loading)
    }
}

/// Small wrapper for the services registry error mapping.
pub fn map_docker_error(err: &DockerError) -> AppError {
    AppError::from(err)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tuxstack_docker_core::{DockerClient, DockerConfig};

    use super::*;

    fn services(host: &str) -> DockerServices {
        let client = DockerClient::connect_with_config(DockerConfig {
            host: Some(host.to_string()),
            ..DockerConfig::default()
        })
        .expect("construct remote client");
        DockerServices::new(Arc::new(client))
    }

    #[test]
    fn endpoint_keys_isolate_resolved_remote_daemons() {
        let first = endpoint_key_for_services(&services("tcp://first.example:2375"));
        let second = endpoint_key_for_services(&services("tcp://second.example:2375"));

        assert_eq!(first.fingerprint, "tcp://first.example:2375");
        assert_eq!(second.fingerprint, "tcp://second.example:2375");
        assert_ne!(first.storage_key(), second.storage_key());
    }

    #[test]
    fn endpoint_key_does_not_invent_unavailable_daemon_identity() {
        let endpoint = endpoint_key_for_services(&services("https://engine.example:2376"));

        assert_eq!(endpoint.daemon_id, None);
        assert_eq!(endpoint.storage_key(), endpoint.fingerprint);
    }
}
