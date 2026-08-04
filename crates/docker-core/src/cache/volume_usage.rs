//! Volume size/ref-count cache with explicit "unknown" semantics.
//!
//! Volume sizes change as content changes, so they are cached with a short
//! TTL (default 45 s). Missing or negative values map to `None` — never to
//! a fabricated `0 B`. Persistent cache entries are used at startup and
//! revalidated in the background (stale-while-revalidate).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cache::persistent::EndpointKey;
use crate::cache::persistent::PersistentCache;

const DEFAULT_TTL: Duration = Duration::from_secs(45);

/// Where a cached usage value came from (used for TTL policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeUsageSource {
    SystemDf,
    VolumeList,
    VolumeInspect,
    PersistentCache,
}

/// Cached usage for one volume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedVolumeUsage {
    pub size_bytes: Option<u64>,
    pub ref_count: Option<u64>,
    pub measured_at: DateTime<Utc>,
    pub source: VolumeUsageSource,
}

impl CachedVolumeUsage {
    /// Create from a known measurement.
    pub fn new(size_bytes: Option<u64>, ref_count: Option<u64>, source: VolumeUsageSource) -> Self {
        Self {
            size_bytes,
            ref_count,
            measured_at: Utc::now(),
            source,
        }
    }

    pub fn is_known(&self) -> bool {
        self.size_bytes.is_some()
    }
}

/// In-memory volume usage cache with TTL, optionally backed by the
/// persistent snapshot cache (endpoint-isolated).
#[derive(Clone)]
pub struct VolumeUsageCache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    ttl: Duration,
    persistent: Option<PersistentCache>,
    endpoint: Option<EndpointKey>,
}

struct Entry {
    usage: CachedVolumeUsage,
    inserted_at: Instant,
}

impl Default for VolumeUsageCache {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl VolumeUsageCache {
    pub fn new(persistent: Option<PersistentCache>, endpoint: Option<EndpointKey>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl: DEFAULT_TTL,
            persistent,
            endpoint,
        }
    }

    pub fn with_ttl(
        persistent: Option<PersistentCache>,
        endpoint: Option<EndpointKey>,
        ttl: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            persistent,
            endpoint,
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Seed from a persistent snapshot (startup hydration).
    pub fn seed(&self, entries: Vec<(String, CachedVolumeUsage)>) {
        let mut inner = self.inner.lock().expect("usage lock");
        for (name, usage) in entries {
            inner.insert(
                name,
                Entry {
                    usage,
                    inserted_at: Instant::now(),
                },
            );
        }
    }

    /// Read a fresh entry: memory first, then the persistent snapshot.
    /// Returns `None` when missing or expired; stale entries are still
    /// readable via [`Self::get_stale`] (stale-while-revalidate).
    pub async fn get(&self, volume_name: &str) -> Option<CachedVolumeUsage> {
        {
            let inner = self.inner.lock().expect("usage lock");
            if let Some(entry) = inner.get(volume_name) {
                if entry.inserted_at.elapsed() < self.ttl {
                    return Some(entry.usage.clone());
                }
            }
        }
        self.get_persistent(volume_name).await
    }

    async fn get_persistent(&self, volume_name: &str) -> Option<CachedVolumeUsage> {
        let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) else {
            return None;
        };
        let rows = tokio::task::spawn_blocking({
            let persistent = persistent.clone();
            let endpoint = endpoint.clone();
            move || persistent.hydrate_usage(&endpoint)
        })
        .await
        .ok()?;
        let row = rows
            .into_iter()
            .find(|(name, _, _, _)| name == volume_name)?;
        let usage = CachedVolumeUsage {
            size_bytes: row.1,
            ref_count: row.2,
            measured_at: chrono::DateTime::from_timestamp(row.3, 0).unwrap_or_else(Utc::now),
            source: VolumeUsageSource::PersistentCache,
        };
        self.inner.lock().expect("usage lock").insert(
            volume_name.to_string(),
            Entry {
                usage: usage.clone(),
                inserted_at: Instant::now(),
            },
        );
        Some(usage)
    }

    /// Read an entry even when stale (stale-while-revalidate display).
    pub fn get_stale(&self, volume_name: &str) -> Option<CachedVolumeUsage> {
        self.inner
            .lock()
            .expect("usage lock")
            .get(volume_name)
            .map(|entry| entry.usage.clone())
    }

    /// Store a fresh measurement in memory and (write-behind) persist it.
    pub async fn put(
        &self,
        volume_name: &str,
        size_bytes: Option<u64>,
        ref_count: Option<u64>,
        source: VolumeUsageSource,
    ) {
        let usage = CachedVolumeUsage::new(size_bytes, ref_count, source);
        self.inner.lock().expect("usage lock").insert(
            volume_name.to_string(),
            Entry {
                usage: usage.clone(),
                inserted_at: Instant::now(),
            },
        );
        if let (Some(persistent), Some(endpoint)) = (&self.persistent, &self.endpoint) {
            let _ = tokio::task::spawn_blocking({
                let persistent = persistent.clone();
                let endpoint = endpoint.clone();
                let volume_name = volume_name.to_string();
                move || persistent.put_usage(&endpoint, &volume_name, size_bytes, ref_count)
            })
            .await;
        }
    }

    /// All entries regardless of age (used for persistence).
    pub fn snapshot(&self) -> Vec<(String, CachedVolumeUsage)> {
        self.inner
            .lock()
            .expect("usage lock")
            .iter()
            .map(|(name, entry)| (name.clone(), entry.usage.clone()))
            .collect()
    }

    pub fn invalidate(&self, volume_name: &str) {
        self.inner.lock().expect("usage lock").remove(volume_name);
    }

    /// Drop entries older than the TTL; returns the number removed.
    pub fn prune_expired(&self) -> usize {
        let mut inner = self.inner.lock().expect("usage lock");
        let before = inner.len();
        inner.retain(|_, entry| entry.inserted_at.elapsed() < self.ttl);
        before - inner.len()
    }

    pub fn clear(&self) {
        self.inner.lock().expect("usage lock").clear();
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("usage lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ttl_expiry() {
        let cache = VolumeUsageCache::with_ttl(None, None, Duration::from_millis(30));
        cache
            .put("vol-a", Some(2048), Some(2), VolumeUsageSource::SystemDf)
            .await;
        assert_eq!(cache.get("vol-a").await.unwrap().size_bytes, Some(2048));

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(
            cache.get("vol-a").await.is_none(),
            "expired entries must vanish"
        );
    }

    #[tokio::test]
    async fn stale_while_revalidate() {
        let cache = VolumeUsageCache::with_ttl(None, None, Duration::from_millis(30));
        cache
            .put("vol-a", Some(2048), Some(2), VolumeUsageSource::SystemDf)
            .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(cache.get("vol-a").await.is_none());
        // Stale value still displayable during background refresh.
        assert_eq!(cache.get_stale("vol-a").unwrap().size_bytes, Some(2048));
    }

    #[tokio::test]
    async fn unknown_size_never_zero() {
        let cache = VolumeUsageCache::new(None, None);
        cache
            .put("vol-a", None, None, VolumeUsageSource::VolumeList)
            .await;
        let usage = cache.get("vol-a").await.unwrap();
        assert!(usage.size_bytes.is_none());
        assert!(!usage.is_known());
    }

    #[tokio::test]
    async fn invalidate_removes() {
        let cache = VolumeUsageCache::new(None, None);
        cache
            .put("vol-a", Some(1), None, VolumeUsageSource::VolumeList)
            .await;
        cache.invalidate("vol-a");
        assert!(cache.get("vol-a").await.is_none());
    }

    #[tokio::test]
    async fn snapshot_round_trip() {
        let cache = VolumeUsageCache::new(None, None);
        cache
            .put("vol-a", Some(10), Some(1), VolumeUsageSource::SystemDf)
            .await;
        cache
            .put("vol-b", None, None, VolumeUsageSource::VolumeList)
            .await;
        let snapshot = cache.snapshot();
        assert_eq!(snapshot.len(), 2);
        let fresh = VolumeUsageCache::new(None, None);
        fresh.seed(snapshot);
        assert_eq!(fresh.get("vol-a").await.unwrap().size_bytes, Some(10));
        assert!(fresh.get("vol-b").await.unwrap().size_bytes.is_none());
    }

    #[tokio::test]
    async fn persistent_backing_hydration() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = crate::cache::persistent::PersistentCacheConfig {
            path: dir.path().join("cache.sqlite3"),
            flush_debounce: Duration::from_millis(10),
        };
        let persistent = PersistentCache::open(&config).expect("open");
        let endpoint = EndpointKey {
            fingerprint: "unix:///var/run/docker.sock".into(),
            daemon_id: None,
            context_name: None,
        };

        let writer = VolumeUsageCache::new(Some(persistent.clone()), Some(endpoint.clone()));
        writer
            .put("vol-x", Some(4096), Some(3), VolumeUsageSource::SystemDf)
            .await;
        std::thread::sleep(Duration::from_millis(60));

        // A fresh cache with the same persistent backing hydrates from disk.
        let reader = VolumeUsageCache::new(Some(persistent), Some(endpoint));
        let usage = reader.get("vol-x").await.expect("hydrated usage");
        assert_eq!(usage.size_bytes, Some(4096));
        assert_eq!(usage.ref_count, Some(3));
    }
}
