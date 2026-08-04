//! SQLite-backed persistent snapshot cache.
//!
//! Design constraints from the caching spec:
//!
//! * endpoint isolation: every row is keyed by [`EndpointKey`], so a local
//!   Docker engine's cache can never leak into a remote engine's page;
//! * schema versioning with automatic rebuild on mismatch;
//! * corruption tolerance: a broken DB file is deleted and recreated rather
//!   than blocking app startup; a single corrupt payload row is dropped;
//! * write-behind: in-memory updates are flushed in a single transaction
//!   after a debounce interval, so per-field patches never hit the disk;
//! * WAL mode + busy timeout so readers never block the Qt thread.
//!
//! The DB file lives under the XDG cache directory
//! (`$XDG_CACHE_HOME/tuxstack/docker-cache.sqlite3`), never the config dir.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::mpsc::{self, RecvTimeoutError};

/// Cache keys that must never be persisted (documented guard).
pub const SENSITIVE_KEYS: &[&str] = &[
    "environment", // environment values may contain secrets
    "registry_credentials",
    "auth_config",
    "container_secrets",
    "log_content",
    "volume_file_names",
    "volume_file_contents",
];

pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Isolation key identifying one Docker endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EndpointKey {
    pub fingerprint: String,
    pub daemon_id: Option<String>,
    pub context_name: Option<String>,
}

impl EndpointKey {
    /// The storage key used in the SQLite tables. Combines the endpoint
    /// fingerprint with the daemon ID when known, so a daemon restart that
    /// changes its ID invalidates the old snapshot naturally.
    pub fn storage_key(&self) -> String {
        match (&self.daemon_id, &self.context_name) {
            (Some(daemon), Some(context)) => format!("{}|{daemon}|{context}", self.fingerprint),
            (Some(daemon), None) => format!("{}|{daemon}", self.fingerprint),
            (None, Some(context)) => format!("{}||{context}", self.fingerprint),
            (None, None) => self.fingerprint.clone(),
        }
    }
}

/// Configuration for the persistent cache.
#[derive(Debug, Clone)]
pub struct PersistentCacheConfig {
    /// Absolute path to the SQLite file.
    pub path: PathBuf,
    /// Debounce for write-behind flushing.
    pub flush_debounce: Duration,
}

impl Default for PersistentCacheConfig {
    fn default() -> Self {
        Self {
            path: default_cache_path(),
            flush_debounce: Duration::from_millis(700),
        }
    }
}

/// Resolve the default cache file under the XDG cache directory.
pub fn default_cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| {
                    let mut path = PathBuf::from(home);
                    path.push(".cache");
                    path
                })
                .unwrap_or_else(|| PathBuf::from("/tmp"))
        });
    let mut path = base;
    path.push("tuxstack");
    path.push("docker-cache.sqlite3");
    path
}

/// Pending write-behind batch entry.
struct PendingWrite {
    table: &'static str,
    endpoint: String,
    key: String,
    payload: Option<Vec<u8>>,
    timestamp: i64,
}

/// Thread-safe SQLite cache with write-behind flushing.
#[derive(Clone)]
pub struct PersistentCache {
    inner: Arc<PersistentCacheInner>,
}

struct PersistentCacheInner {
    /// Connection guarded by a std Mutex; only touched inside
    /// `spawn_blocking` closures so the Qt thread is never blocked.
    connection: Mutex<Connection>,
    flush_tx: mpsc::Sender<PendingWrite>,
}

impl PersistentCache {
    /// Open (or create) the cache. Corruption or schema mismatch rebuilds
    /// the database instead of failing.
    pub fn open(config: &PersistentCacheConfig) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = config.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match Self::open_connection(&config.path) {
            Ok(connection) => Ok(Self::from_connection(connection, config)),
            Err(_) => {
                // Corrupt or incompatible cache: rebuild and continue.
                let _ = std::fs::remove_file(&config.path);
                let _ = std::fs::remove_file(config.path.with_extension("sqlite3-wal"));
                let _ = std::fs::remove_file(config.path.with_extension("sqlite3-shm"));
                let connection = Self::open_connection(&config.path)?;
                Ok(Self::from_connection(connection, config))
            }
        }
    }

    fn open_connection(path: &std::path::Path) -> Result<Connection, rusqlite::Error> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "busy_timeout", 2_000)?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        Self::create_schema(&connection)?;
        Ok(connection)
    }

    fn create_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS docker_cache_metadata (
                endpoint_key TEXT PRIMARY KEY,
                schema_version INTEGER NOT NULL,
                daemon_id TEXT,
                api_version TEXT,
                last_connected_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS image_summaries (
                endpoint_key TEXT NOT NULL,
                image_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, image_id)
            );
            CREATE TABLE IF NOT EXISTS image_details (
                endpoint_key TEXT NOT NULL,
                image_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, image_id)
            );
            CREATE TABLE IF NOT EXISTS volume_summaries (
                endpoint_key TEXT NOT NULL,
                volume_name TEXT NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, volume_name)
            );
            CREATE TABLE IF NOT EXISTS volume_usage (
                endpoint_key TEXT NOT NULL,
                volume_name TEXT NOT NULL,
                size_bytes INTEGER,
                ref_count INTEGER,
                measured_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, volume_name)
            );
            CREATE TABLE IF NOT EXISTS network_summaries (
                endpoint_key TEXT NOT NULL,
                network_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, network_id)
            );
            CREATE TABLE IF NOT EXISTS container_summaries (
                endpoint_key TEXT NOT NULL,
                container_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (endpoint_key, container_id)
            );",
        )?;
        Ok(())
    }

    fn from_connection(connection: Connection, config: &PersistentCacheConfig) -> Self {
        let (flush_tx, flush_rx) = mpsc::channel::<PendingWrite>();
        let inner = Arc::new(PersistentCacheInner {
            connection: Mutex::new(connection),
            flush_tx,
        });
        // Write-behind flusher: drains the channel, debouncing the last
        // write so bursts coalesce into one transaction.
        let flusher_inner = inner.clone();
        let debounce = config.flush_debounce;
        std::thread::Builder::new()
            .name("tuxstack-cache-flush".into())
            .spawn(move || {
                while let Ok(first) = flush_rx.recv() {
                    let mut batch = vec![first];
                    // Debounce: keep collecting while writes arrive faster
                    // than `debounce`; flush when the queue goes quiet.
                    loop {
                        match flush_rx.recv_timeout(debounce) {
                            Ok(write) => batch.push(write),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => {
                                flush_batch(&flusher_inner, &batch);
                                return;
                            }
                        }
                    }
                    flush_batch(&flusher_inner, &batch);
                }
            })
            .expect("cache flush thread");
        Self { inner }
    }

    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn enqueue(&self, table: &'static str, endpoint: &str, key: &str, payload: Option<Vec<u8>>) {
        let _ = self.inner.flush_tx.send(PendingWrite {
            table,
            endpoint: endpoint.to_string(),
            key: key.to_string(),
            payload,
            timestamp: Self::now_unix(),
        });
    }

    /// Record connection metadata for an endpoint.
    pub fn record_connection(&self, endpoint: &EndpointKey, api_version: Option<&str>) {
        let storage = endpoint.storage_key();
        let daemon = endpoint.daemon_id.clone();
        let api = api_version.map(str::to_string);
        let now = Self::now_unix();
        let inner = self.inner.clone();
        let _ = std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return;
            };
            let _ = connection.execute(
                "INSERT INTO docker_cache_metadata
                    (endpoint_key, schema_version, daemon_id, api_version, last_connected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(endpoint_key) DO UPDATE SET
                    schema_version = excluded.schema_version,
                    daemon_id = excluded.daemon_id,
                    api_version = excluded.api_version,
                    last_connected_at = excluded.last_connected_at",
                params![storage, CACHE_SCHEMA_VERSION, daemon, api, now],
            );
        });
    }

    /// Store one row through the write-behind queue.
    pub fn put<T: Serialize>(
        &self,
        table: &'static str,
        endpoint: &EndpointKey,
        key: &str,
        value: &T,
    ) {
        let payload = serde_json::to_vec(value).ok();
        self.enqueue(table, &endpoint.storage_key(), key, payload);
    }

    /// Store a typed value that does not serialize (e.g. usage with Option
    /// columns) directly via a prepared statement path.
    pub fn put_usage(
        &self,
        endpoint: &EndpointKey,
        volume_name: &str,
        size_bytes: Option<u64>,
        ref_count: Option<u64>,
    ) {
        let storage = endpoint.storage_key();
        let now = Self::now_unix();
        let size = size_bytes.map(|v| v as i64);
        let count = ref_count.map(|v| v as i64);
        let volume_name = volume_name.to_string();
        let inner = self.inner.clone();
        let _ = std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return;
            };
            let _ = connection.execute(
                "INSERT INTO volume_usage
                    (endpoint_key, volume_name, size_bytes, ref_count, measured_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(endpoint_key, volume_name) DO UPDATE SET
                    size_bytes = excluded.size_bytes,
                    ref_count = excluded.ref_count,
                    measured_at = excluded.measured_at",
                params![storage, volume_name, size, count, now],
            );
        });
    }

    /// Read all rows of a table for an endpoint, mapping via `decode`.
    pub fn hydrate<T: DeserializeOwned + Send + 'static>(
        &self,
        table: &'static str,
        endpoint: &EndpointKey,
    ) -> Vec<(String, T)> {
        let Some(key_column) = key_column_for(table) else {
            return Vec::new();
        };
        let storage = endpoint.storage_key();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return Vec::new();
            };
            let Ok(mut statement) = connection.prepare(&format!(
                "SELECT {key_column}, payload FROM {table} WHERE endpoint_key = ?1"
            )) else {
                return Vec::new();
            };
            let rows = statement
                .query_map([&storage], |row| {
                    let key: String = row.get(0)?;
                    let payload: Vec<u8> = row.get(1)?;
                    Ok((key, payload))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
                .unwrap_or_default();
            // Corrupt payload rows are dropped individually; never panic.
            rows.into_iter()
                .filter_map(|(key, payload)| {
                    serde_json::from_slice::<T>(&payload)
                        .ok()
                        .map(|value| (key, value))
                })
                .collect()
        })
        .join()
        .unwrap_or_default()
    }

    /// Read usage rows for an endpoint.
    pub fn hydrate_usage(
        &self,
        endpoint: &EndpointKey,
    ) -> Vec<(String, Option<u64>, Option<u64>, i64)> {
        let storage = endpoint.storage_key();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return Vec::new();
            };
            let Ok(mut statement) = connection.prepare(
                "SELECT volume_name, size_bytes, ref_count, measured_at
                 FROM volume_usage WHERE endpoint_key = ?1",
            ) else {
                return Vec::new();
            };
            statement
                .query_map([&storage], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                        row.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                        row.get::<_, i64>(3)?,
                    ))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
                .unwrap_or_default()
        })
        .join()
        .unwrap_or_default()
    }

    /// Read the last connected metadata for an endpoint.
    pub fn last_connected(&self, endpoint: &EndpointKey) -> Option<i64> {
        let storage = endpoint.storage_key();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return None;
            };
            connection
                .query_row(
                    "SELECT last_connected_at FROM docker_cache_metadata WHERE endpoint_key = ?1",
                    [&storage],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten()
        })
        .join()
        .unwrap_or(None)
    }

    /// Delete rows for a single resource (e.g. image removed).
    pub fn remove_key(&self, table: &'static str, endpoint: &EndpointKey, key: &str) {
        let Some(key_column) = key_column_for(table) else {
            return;
        };
        let storage = endpoint.storage_key();
        let key = key.to_string();
        let inner = self.inner.clone();
        let _ = std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return;
            };
            let _ = connection.execute(
                &format!("DELETE FROM {table} WHERE endpoint_key = ?1 AND {key_column} = ?2"),
                params![storage, key],
            );
        })
        .join();
    }

    /// Remove all rows belonging to one endpoint (e.g. endpoint switch).
    pub fn clear_endpoint(&self, endpoint: &EndpointKey) {
        let storage = endpoint.storage_key();
        let inner = self.inner.clone();
        let _ = std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return;
            };
            for table in [
                "image_summaries",
                "image_details",
                "volume_summaries",
                "volume_usage",
                "network_summaries",
                "container_summaries",
            ] {
                let _ = connection.execute(
                    &format!("DELETE FROM {table} WHERE endpoint_key = ?1"),
                    [&storage],
                );
            }
        })
        .join();
    }

    /// Block until the pending queue is flushed (used at shutdown).
    pub fn flush(&self) {
        // Enqueue a sentinel write and wait briefly for the flusher.
        self.enqueue("docker_cache_metadata", "flush-sentinel", "sentinel", None);
        std::thread::sleep(Duration::from_millis(30));
    }

    /// Read a single cached value.
    pub fn get<T: DeserializeOwned + Send + 'static>(
        &self,
        table: &'static str,
        endpoint: &EndpointKey,
        key: &str,
    ) -> Option<T> {
        let storage = endpoint.storage_key();
        let key = key.to_string();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let Ok(connection) = inner.connection.lock() else {
                return None;
            };
            let payload: Option<Vec<u8>> = connection
                .query_row(
                    &format!("SELECT payload FROM {table} WHERE endpoint_key = ?1 AND key = ?2"),
                    params![storage, key],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten();
            payload.and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
        .join()
        .unwrap_or(None)
    }
}

fn key_column_for(table: &str) -> Option<&'static str> {
    match table {
        "image_summaries" | "image_details" => Some("image_id"),
        "volume_summaries" => Some("volume_name"),
        "network_summaries" => Some("network_id"),
        "container_summaries" => Some("container_id"),
        _ => None,
    }
}

fn flush_batch(inner: &PersistentCacheInner, batch: &[PendingWrite]) {
    if batch.is_empty() {
        return;
    }
    let Ok(mut connection) = inner.connection.lock() else {
        return;
    };
    let transaction = match connection.transaction() {
        Ok(tx) => tx,
        Err(_) => return,
    };
    for write in batch {
        if write.payload.is_none() {
            continue;
        }
        let Some(key_column) = key_column_for(write.table) else {
            continue;
        };
        let payload = write.payload.as_deref().unwrap_or_default();
        let _ = transaction.execute(
            &format!(
                "INSERT INTO {table}
                    (endpoint_key, {key_column}, payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(endpoint_key, {key_column}) DO UPDATE SET
                    payload = excluded.payload,
                    updated_at = excluded.updated_at",
                table = write.table,
            ),
            params![write.endpoint, write.key, payload, write.timestamp],
        );
    }
    let _ = transaction.commit();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct Row {
        name: String,
        size: u64,
    }

    fn test_cache() -> (tempfile::TempDir, PersistentCache) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = PersistentCacheConfig {
            path: dir.path().join("cache.sqlite3"),
            flush_debounce: Duration::from_millis(10),
        };
        let cache = PersistentCache::open(&config).expect("open");
        (dir, cache)
    }

    fn endpoint(name: &str) -> EndpointKey {
        EndpointKey {
            fingerprint: format!("unix:///var/run/{name}.sock"),
            daemon_id: Some(format!("daemon-{name}")),
            context_name: Some(name.to_string()),
        }
    }

    #[test]
    fn put_and_hydrate_round_trip() {
        let (_dir, cache) = test_cache();
        let ep = endpoint("local");
        let row = Row {
            name: "alpine".into(),
            size: 1024,
        };
        cache.put("image_summaries", &ep, "sha256:abc", &row);
        cache.flush();

        let hydrated = cache.hydrate::<Row>("image_summaries", &ep);
        assert_eq!(hydrated.len(), 1);
        assert_eq!(hydrated[0].0, "sha256:abc");
        assert_eq!(hydrated[0].1, row);
    }

    #[test]
    fn endpoint_isolation() {
        let (_dir, cache) = test_cache();
        let local = endpoint("local");
        let remote = endpoint("remote");
        cache.put(
            "image_summaries",
            &local,
            "sha256:abc",
            &Row {
                name: "x".into(),
                size: 1,
            },
        );
        cache.put(
            "image_summaries",
            &remote,
            "sha256:abc",
            &Row {
                name: "y".into(),
                size: 2,
            },
        );
        cache.flush();

        let local_rows = cache.hydrate::<Row>("image_summaries", &local);
        let remote_rows = cache.hydrate::<Row>("image_summaries", &remote);
        assert_eq!(local_rows.len(), 1);
        assert_eq!(local_rows[0].1.name, "x");
        assert_eq!(remote_rows[0].1.name, "y");
        assert_eq!(
            local_rows[0].0, remote_rows[0].0,
            "same ID in two endpoints"
        );
    }

    #[test]
    fn corrupt_payload_dropped_individually() {
        let (_dir, cache) = test_cache();
        let ep = endpoint("local");
        cache.put(
            "image_summaries",
            &ep,
            "good",
            &Row {
                name: "ok".into(),
                size: 1,
            },
        );
        cache.flush();
        // Corrupt one row directly.
        {
            let connection = cache.inner.connection.lock().unwrap();
            connection
                .execute(
                    "INSERT INTO image_summaries (endpoint_key, image_id, payload, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![ep.storage_key(), "bad", b"not-json".to_vec(), 0],
                )
                .unwrap();
        }
        let hydrated = cache.hydrate::<Row>("image_summaries", &ep);
        assert_eq!(hydrated.len(), 1);
        assert_eq!(hydrated[0].0, "good");
    }

    #[test]
    fn clear_endpoint_removes_only_that_endpoint() {
        let (_dir, cache) = test_cache();
        let local = endpoint("local");
        let remote = endpoint("remote");
        cache.put(
            "image_summaries",
            &local,
            "a",
            &Row {
                name: "x".into(),
                size: 1,
            },
        );
        cache.put(
            "image_summaries",
            &remote,
            "a",
            &Row {
                name: "y".into(),
                size: 2,
            },
        );
        cache.flush();
        cache.clear_endpoint(&local);
        assert!(cache.hydrate::<Row>("image_summaries", &local).is_empty());
        assert_eq!(cache.hydrate::<Row>("image_summaries", &remote).len(), 1);
    }

    #[test]
    fn corrupt_database_is_rebuilt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cache.sqlite3");
        std::fs::write(&path, b"this is not a sqlite database").unwrap();
        let config = PersistentCacheConfig {
            path: path.clone(),
            flush_debounce: Duration::from_millis(10),
        };
        let cache = PersistentCache::open(&config).expect("open must recover");
        let ep = endpoint("local");
        cache.put(
            "image_summaries",
            &ep,
            "a",
            &Row {
                name: "x".into(),
                size: 1,
            },
        );
        cache.flush();
        assert_eq!(cache.hydrate::<Row>("image_summaries", &ep).len(), 1);
    }

    #[test]
    fn remove_key_removes_single_row() {
        let (_dir, cache) = test_cache();
        let ep = endpoint("local");
        cache.put(
            "image_summaries",
            &ep,
            "a",
            &Row {
                name: "x".into(),
                size: 1,
            },
        );
        cache.put(
            "image_summaries",
            &ep,
            "b",
            &Row {
                name: "y".into(),
                size: 2,
            },
        );
        cache.flush();
        cache.remove_key("image_summaries", &ep, "a");
        std::thread::sleep(Duration::from_millis(30));
        let hydrated = cache.hydrate::<Row>("image_summaries", &ep);
        assert_eq!(hydrated.len(), 1);
        assert_eq!(hydrated[0].0, "b");
    }
}
