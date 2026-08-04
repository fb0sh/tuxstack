//! Long-lived image metadata cache keyed by content-addressed image ID.
//!
//! An image ID is content-addressed, so basic inspect-derived metadata
//! (architecture, OS, variant, config digest) is stable for the lifetime of
//! that image. This cache lets the list show real platform values without
//! inspecting every image on every refresh or restart. Metadata is stored in
//! memory and optionally persisted to the SQLite snapshot cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};

use crate::cache::persistent::EndpointKey;
use crate::cache::persistent::PersistentCache;

const IMAGE_DETAILS_TABLE: &str = "image_details";
const PREFETCH_CONCURRENCY: usize = 4;

/// Cached metadata for one image ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedImageMetadata {
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub variant: Option<String>,
    pub config_digest: Option<String>,
    /// Unix timestamp of the inspect that produced this metadata.
    pub inspected_at: i64,
}

/// In-memory image metadata cache, optionally backed by the persistent
/// snapshot cache (endpoint-isolated). Prefetch uses bounded concurrency;
/// deduplication of concurrent inspects is the caller's responsibility via
/// [`super::RequestCoordinator`] where needed.
#[derive(Clone)]
pub struct ImageMetadataCache {
    inner: Arc<Mutex<HashMap<String, CachedImageMetadata>>>,
    persistent: Option<PersistentCache>,
    endpoint: Option<EndpointKey>,
}

impl Default for ImageMetadataCache {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl ImageMetadataCache {
    pub fn new(persistent: Option<PersistentCache>, endpoint: Option<EndpointKey>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            persistent,
            endpoint,
        }
    }

    /// Seed from a persisted snapshot (startup hydration). Existing entries
    /// with the same image ID are overwritten.
    pub fn seed(&self, entries: Vec<(String, CachedImageMetadata)>) {
        let mut inner = self.inner.lock().expect("metadata lock");
        for (image_id, entry) in entries {
            inner.insert(image_id, entry);
        }
    }

    /// All entries (used for persistence).
    pub fn snapshot(&self) -> Vec<(String, CachedImageMetadata)> {
        self.inner
            .lock()
            .expect("metadata lock")
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect()
    }

    /// Read metadata for an image ID: memory first, then the persistent
    /// snapshot. The persistent read happens off the async side so the Qt
    /// thread never blocks.
    pub async fn get(&self, image_id: &str) -> Option<CachedImageMetadata> {
        if let Some(entry) = self.inner.lock().expect("metadata lock").get(image_id) {
            return Some(entry.clone());
        }
        self.get_persistent(image_id).await
    }

    /// Synchronous memory-only read (tests and non-async callers).
    pub fn get_sync(&self, image_id: &str) -> Option<CachedImageMetadata> {
        self.inner
            .lock()
            .expect("metadata lock")
            .get(image_id)
            .cloned()
    }

    async fn get_persistent(&self, image_id: &str) -> Option<CachedImageMetadata> {
        let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) else {
            return None;
        };
        let entry = tokio::task::spawn_blocking({
            let persistent = persistent.clone();
            let endpoint = endpoint.clone();
            let image_id = image_id.to_string();
            move || persistent.get::<CachedImageMetadata>(IMAGE_DETAILS_TABLE, &endpoint, &image_id)
        })
        .await
        .ok()
        .flatten()?;
        // Promote the persistent hit into memory.
        self.inner
            .lock()
            .expect("metadata lock")
            .insert(image_id.to_string(), entry.clone());
        Some(entry)
    }

    /// Insert or replace; returns the previous value if any. The entry is
    /// also written to the persistent cache (write-behind).
    pub fn insert(&self, image_id: &str, entry: CachedImageMetadata) {
        self.inner
            .lock()
            .expect("metadata lock")
            .insert(image_id.to_string(), entry.clone());
        if let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) {
            persistent.put(IMAGE_DETAILS_TABLE, endpoint, image_id, &entry);
        }
    }

    pub fn remove(&self, image_id: &str) {
        self.inner.lock().expect("metadata lock").remove(image_id);
        if let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) {
            persistent.remove_key(IMAGE_DETAILS_TABLE, endpoint, image_id);
        }
    }

    pub fn contains(&self, image_id: &str) -> bool {
        self.inner
            .lock()
            .expect("metadata lock")
            .contains_key(image_id)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("metadata lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Collect metadata for the IDs missing from the cache (prefetch targets).
    pub fn missing(&self, image_ids: &[String]) -> Vec<String> {
        let inner = self.inner.lock().expect("metadata lock");
        image_ids
            .iter()
            .filter(|id| !inner.contains_key(*id))
            .cloned()
            .collect()
    }

    /// Drop entries whose image IDs are no longer present (e.g. image removed).
    pub fn retain(&self, image_ids: &[String]) {
        let mut inner = self.inner.lock().expect("metadata lock");
        let present: std::collections::HashSet<&String> = image_ids.iter().collect();
        let removed: Vec<String> = inner
            .keys()
            .filter(|id| !present.contains(id))
            .cloned()
            .collect();
        inner.retain(|id, _| present.contains(id));
        if let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) {
            for id in removed {
                persistent.remove_key(IMAGE_DETAILS_TABLE, endpoint, &id);
            }
        }
    }

    /// Prefetch metadata for missing IDs with bounded concurrency. Returns
    /// the number of IDs that are still missing after the pass (failed
    /// fetches are not cached, so they may be retried later).
    pub async fn prefetch<F, Fut>(&self, image_ids: &[String], fetcher: F) -> usize
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<CachedImageMetadata, String>> + Send + 'static,
    {
        let missing = self.missing(image_ids);
        if missing.is_empty() {
            return 0;
        }
        let cache = self.clone();
        let results: Vec<(String, Result<CachedImageMetadata, String>)> = stream::iter(missing)
            .map(|image_id| {
                let fetcher = &fetcher;
                async move {
                    let outcome = fetcher(image_id.clone()).await;
                    (image_id, outcome)
                }
            })
            .buffer_unordered(PREFETCH_CONCURRENCY)
            .collect()
            .await;
        for (image_id, outcome) in results {
            if let Ok(entry) = outcome {
                cache.insert(&image_id, entry);
            }
        }
        self.missing(image_ids).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn entry(arch: &str) -> CachedImageMetadata {
        CachedImageMetadata {
            architecture: Some(arch.into()),
            os: Some("linux".into()),
            variant: None,
            config_digest: None,
            inspected_at: 1,
        }
    }

    #[test]
    fn insert_get_remove() {
        let cache = ImageMetadataCache::new(None, None);
        assert!(cache.is_empty());
        cache.insert("sha256:abc", entry("amd64"));
        assert_eq!(
            cache
                .get_sync("sha256:abc")
                .unwrap()
                .architecture
                .as_deref(),
            Some("amd64")
        );
        assert!(cache.contains("sha256:abc"));
        cache.remove("sha256:abc");
        assert!(!cache.contains("sha256:abc"));
    }

    #[test]
    fn missing_reports_only_absent_ids() {
        let cache = ImageMetadataCache::new(None, None);
        cache.insert("sha256:a", entry("amd64"));
        let missing = cache.missing(&["sha256:a".into(), "sha256:b".into()]);
        assert_eq!(missing, vec!["sha256:b".to_string()]);
    }

    #[test]
    fn retain_drops_vanished_ids() {
        let cache = ImageMetadataCache::new(None, None);
        cache.insert("sha256:a", entry("amd64"));
        cache.insert("sha256:b", entry("arm64"));
        cache.retain(&["sha256:a".into()]);
        assert!(cache.contains("sha256:a"));
        assert!(!cache.contains("sha256:b"));
    }

    #[test]
    fn seed_overwrites_same_ids_keeps_others() {
        let cache = ImageMetadataCache::new(None, None);
        cache.insert("sha256:a", entry("amd64"));
        cache.seed(vec![
            ("sha256:c".to_string(), entry("arm64")),
            ("sha256:a".to_string(), entry("riscv64")),
        ]);
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get_sync("sha256:a").unwrap().architecture.as_deref(),
            Some("riscv64")
        );
        assert!(cache.contains("sha256:c"));
    }

    #[tokio::test]
    async fn prefetch_fills_missing_and_reports_remaining() {
        let cache = ImageMetadataCache::new(None, None);
        cache.insert("sha256:have", entry("amd64"));
        let calls = Arc::new(AtomicUsize::new(0));

        let remaining = cache
            .prefetch(&["sha256:have".into(), "sha256:need".into()], {
                let calls = calls.clone();
                move |id| {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        if id == "sha256:need" {
                            Ok(entry("arm64"))
                        } else {
                            Err("boom".into())
                        }
                    }
                }
            })
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "only missing fetched");
        assert_eq!(remaining, 0);
        assert_eq!(
            cache
                .get_sync("sha256:need")
                .unwrap()
                .architecture
                .as_deref(),
            Some("arm64")
        );
    }

    #[tokio::test]
    async fn failed_fetch_not_cached_and_retryable() {
        let cache = ImageMetadataCache::new(None, None);
        let calls = Arc::new(AtomicUsize::new(0));
        let remaining = cache
            .prefetch(&["sha256:f".into()], {
                let calls = calls.clone();
                move |_| {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Err("temporary".into())
                    }
                }
            })
            .await;
        assert_eq!(remaining, 1, "failed fetch stays missing");
        assert!(!cache.contains("sha256:f"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
