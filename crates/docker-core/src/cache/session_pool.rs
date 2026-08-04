//! Preview session pool and per-directory cache.
//!
//! Creating a helper container is expensive (image check, container create,
//! start). Switching between Info and Files tabs, or between resources,
//! should never rebuild a session that still exists. This pool keeps a
//! bounded number of live sessions keyed by resource name with an idle TTL
//! and LRU eviction, and caches recently listed directory contents (memory
//! only — filenames are sensitive and never persisted).
//!
//! The pool is generic over the session type so volume preview helpers and
//! image preview helpers share the same bookkeeping; the default type
//! parameter uses `FilesystemSession` from the unified filesystem service.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::services::filesystem::types::{FilesystemEntry, FilesystemSession};

/// Derive the pool key (volume name / image id) from a preview session.
pub trait PoolKey {
    fn pool_key(&self) -> &str;
}

impl PoolKey for FilesystemSession {
    fn pool_key(&self) -> &str {
        match &self.source {
            crate::services::filesystem::types::FilesystemSource::Image { image_id, .. } => image_id,
            crate::services::filesystem::types::FilesystemSource::Volume { volume_name } => volume_name,
        }
    }
}

/// State of a pooled session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSessionState {
    /// Currently being used by a page.
    Active,
    /// No page is using it but it is kept for reuse.
    Idle,
}

/// A cached preview session with its own directory cache.
#[derive(Debug, Clone)]
pub struct CachedPreviewSession<S = FilesystemSession> {
    pub session: S,
    pub state: PreviewSessionState,
    pub last_used_at: Instant,
}

/// Cached directory listing for one path.
#[derive(Debug, Clone)]
pub struct CachedDirectory {
    pub entries: Arc<Vec<FilesystemEntry>>,
    pub fetched_at: Instant,
    pub generation: u64,
}

/// Configuration for the session pool.
#[derive(Debug, Clone)]
pub struct PreviewSessionPoolConfig {
    /// Maximum live sessions (active + idle).
    pub max_sessions: usize,
    /// Idle sessions are stopped after this long without use.
    pub idle_ttl: Duration,
    /// Directory listings are re-read after this long (stale-while-revalidate).
    pub directory_ttl: Duration,
}

impl Default for PreviewSessionPoolConfig {
    fn default() -> Self {
        Self {
            max_sessions: 3,
            idle_ttl: Duration::from_secs(120),
            directory_ttl: Duration::from_secs(5),
        }
    }
}

/// Session pool + directory cache shared by the file-browsing pages.
#[derive(Clone)]
pub struct PreviewSessionPool<S = FilesystemSession> {
    inner: Arc<Mutex<PoolInner<S>>>,
    config: PreviewSessionPoolConfig,
}

struct PoolInner<S> {
    sessions: HashMap<String, CachedPreviewSession<S>>,
    /// LRU order for idle eviction: front = least recently used.
    lru: VecDeque<String>,
    directories: HashMap<(String, String), CachedDirectory>,
    generation: u64,
}

impl<S> Default for PoolInner<S> {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            lru: VecDeque::new(),
            directories: HashMap::new(),
            generation: 0,
        }
    }
}

impl Default for PreviewSessionPool {
    fn default() -> Self {
        Self::with_config(PreviewSessionPoolConfig::default())
    }
}

impl<S> PreviewSessionPool<S> {
    pub fn new() -> Self {
        Self::with_config(PreviewSessionPoolConfig::default())
    }

    pub fn with_config(config: PreviewSessionPoolConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PoolInner::default())),
            config,
        }
    }

    pub fn config(&self) -> &PreviewSessionPoolConfig {
        &self.config
    }

    /// Return a live session for `key` (volume name or image id), marking it active.
    pub async fn acquire(&self, key: &str) -> Option<S>
    where
        S: Clone,
    {
        let mut inner = self.inner.lock().await;
        let entry = inner.sessions.get_mut(key)?;
        entry.state = PreviewSessionState::Active;
        entry.last_used_at = Instant::now();
        let session = inner.sessions.get(key)?.session.clone();
        let name = key.to_string();
        inner.lru.retain(|existing| existing != &name);
        Some(session)
    }

    /// Check whether a live session exists without marking it active.
    pub async fn contains(&self, key: &str) -> bool {
        self.inner.lock().await.sessions.contains_key(key)
    }

    /// Insert a freshly created session.
    pub async fn insert(&self, session: S)
    where
        S: Clone + PoolKey,
    {
        let mut inner = self.inner.lock().await;
        let key = session.pool_key().to_string();
        inner.sessions.insert(
            key.clone(),
            CachedPreviewSession {
                session,
                state: PreviewSessionState::Active,
                last_used_at: Instant::now(),
            },
        );
        inner.lru.retain(|existing| existing != &key);
        // Enforce the session cap: evict the least-recently-used idle session.
        while inner.sessions.len() > self.config.max_sessions {
            let evict = inner
                .lru
                .iter()
                .find(|name| {
                    inner
                        .sessions
                        .get(*name)
                        .map(|s| s.state == PreviewSessionState::Idle)
                        .unwrap_or(false)
                })
                .cloned();
            let Some(evict) = evict else {
                break;
            };
            inner.sessions.remove(&evict);
            inner.lru.retain(|existing| existing != &evict);
        }
    }

    /// Mark a session idle (page switched away) without destroying it.
    pub async fn release(&self, key: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.sessions.get_mut(key) {
            entry.state = PreviewSessionState::Idle;
            entry.last_used_at = Instant::now();
        }
        let name = key.to_string();
        inner.lru.retain(|existing| existing != &name);
        inner.lru.push_back(name);
    }

    /// Remove a session from the pool; returns it for cleanup by the caller.
    pub async fn take(&self, key: &str) -> Option<S> {
        let mut inner = self.inner.lock().await;
        let session = inner.sessions.remove(key)?.session;
        let name = key.to_string();
        inner.lru.retain(|existing| existing != &name);
        inner.directories.retain(|(name, _), _| name != key);
        Some(session)
    }

    /// Stop idle sessions whose idle TTL has expired. Returns the list of
    /// sessions the caller must stop (containers to remove).
    pub async fn evict_expired(&self) -> Vec<S> {
        let mut inner = self.inner.lock().await;
        let mut to_stop = Vec::new();
        let expired: Vec<String> = inner
            .sessions
            .iter()
            .filter(|(_, entry)| {
                entry.state == PreviewSessionState::Idle
                    && entry.last_used_at.elapsed() >= self.config.idle_ttl
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in expired {
            if let Some(entry) = inner.sessions.remove(&name) {
                inner.lru.retain(|existing| existing != &name);
                to_stop.push(entry.session);
            }
        }
        to_stop
    }

    /// Return every live session for shutdown cleanup.
    pub async fn drain_all(&self) -> Vec<S> {
        let mut inner = self.inner.lock().await;
        let sessions: Vec<S> = inner
            .sessions
            .drain()
            .map(|(_, entry)| entry.session)
            .collect();
        inner.lru.clear();
        inner.directories.clear();
        sessions
    }

    pub async fn session_count(&self) -> usize {
        self.inner.lock().await.sessions.len()
    }

    pub async fn active_count(&self) -> usize {
        let inner = self.inner.lock().await;
        inner
            .sessions
            .values()
            .filter(|s| s.state == PreviewSessionState::Active)
            .count()
    }

    // ---- directory cache ----

    /// Return a cached directory listing if fresh within the directory TTL.
    pub async fn directory_hit(
        &self,
        resource_key: &str,
        path: &str,
    ) -> Option<Arc<Vec<FilesystemEntry>>> {
        let mut inner = self.inner.lock().await;
        let key = (resource_key.to_string(), path.to_string());
        let entry = inner.directories.get(&key)?;
        if entry.fetched_at.elapsed() >= self.config.directory_ttl {
            inner.directories.remove(&key);
            return None;
        }
        Some(entry.entries.clone())
    }

    /// Store a fresh directory listing and bump the generation.
    pub async fn directory_put(
        &self,
        resource_key: &str,
        path: &str,
        entries: Vec<FilesystemEntry>,
    ) {
        let mut inner = self.inner.lock().await;
        inner.generation = inner.generation.wrapping_add(1);
        let generation = inner.generation;
        inner.directories.insert(
            (resource_key.to_string(), path.to_string()),
            CachedDirectory {
                entries: Arc::new(entries),
                fetched_at: Instant::now(),
                generation,
            },
        );
    }

    pub async fn directory_count(&self) -> usize {
        self.inner.lock().await.directories.len()
    }

    pub async fn generation(&self) -> u64 {
        self.inner.lock().await.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::filesystem::types::{FilesystemSource, FilesystemSession};
    use chrono::Utc;

    fn session(name: &str, id: u32) -> FilesystemSession {
        FilesystemSession {
            container_id: format!("container-{id}"),
            container_name: format!("tuxstack-fs-helper-{id}"),
            source: FilesystemSource::Volume {
                volume_name: name.to_string(),
            },
            root: "/volume".into(),
            helper_path: "/usr/local/bin/tuxstack-fs-helper".into(),
            protocol_version: 1,
            helper_version: "0.1.0".into(),
            read_only: true,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn acquire_returns_existing_session() {
        let pool = PreviewSessionPool::new();
        pool.insert(session("data", 1)).await;
        assert!(pool.acquire("data").await.is_some());
        assert!(pool.acquire("missing").await.is_none());
        assert_eq!(pool.session_count().await, 1);
        assert_eq!(pool.active_count().await, 1);
    }

    #[tokio::test]
    async fn release_keeps_session_for_reuse() {
        let pool = PreviewSessionPool::new();
        pool.insert(session("data", 1)).await;
        pool.release("data").await;
        // Re-acquire reuses the same session, no new helper.
        assert_eq!(
            pool.acquire("data").await.unwrap().container_id,
            "container-1"
        );
        assert_eq!(pool.session_count().await, 1);
    }

    #[tokio::test]
    async fn lru_evicts_idle_session_when_over_cap() {
        let config = PreviewSessionPoolConfig {
            max_sessions: 2,
            ..Default::default()
        };
        let pool = PreviewSessionPool::with_config(config);
        pool.insert(session("a", 1)).await;
        pool.insert(session("b", 2)).await;
        pool.release("a").await;
        pool.insert(session("c", 3)).await; // over cap -> evict idle "a"
        assert!(!pool.contains("a").await);
        assert!(pool.contains("b").await);
        assert!(pool.contains("c").await);
        assert_eq!(pool.session_count().await, 2);
    }

    #[tokio::test]
    async fn evict_expired_stops_idle_sessions() {
        let config = PreviewSessionPoolConfig {
            idle_ttl: Duration::from_millis(30),
            ..Default::default()
        };
        let pool = PreviewSessionPool::with_config(config);
        pool.insert(session("a", 1)).await;
        pool.release("a").await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let stopped = pool.evict_expired().await;
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].container_id, "container-1");
        assert_eq!(pool.session_count().await, 0);
    }

    #[tokio::test]
    async fn drain_all_returns_everything_for_shutdown() {
        let pool = PreviewSessionPool::new();
        pool.insert(session("a", 1)).await;
        pool.insert(session("b", 2)).await;
        assert_eq!(pool.drain_all().await.len(), 2);
        assert_eq!(pool.session_count().await, 0);
    }

    #[tokio::test]
    async fn directory_cache_hit_and_ttl() {
        let config = PreviewSessionPoolConfig {
            directory_ttl: Duration::from_millis(40),
            ..Default::default()
        };
        let pool: PreviewSessionPool<FilesystemSession> = PreviewSessionPool::with_config(config);
        assert!(pool.directory_hit("data", "/").await.is_none());
        pool.directory_put("data", "/", vec![]).await;
        assert!(pool.directory_hit("data", "/").await.is_some());
        // Different volume/path must not hit.
        assert!(pool.directory_hit("data", "/sub").await.is_none());
        assert!(pool.directory_hit("other", "/").await.is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(pool.directory_hit("data", "/").await.is_none());
    }

    #[tokio::test]
    async fn take_removes_session_and_directory() {
        let pool = PreviewSessionPool::new();
        pool.insert(session("a", 1)).await;
        pool.directory_put("a", "/", vec![]).await;
        assert!(pool.take("a").await.is_some());
        assert_eq!(pool.session_count().await, 0);
        assert_eq!(pool.directory_count().await, 0);
    }
}
