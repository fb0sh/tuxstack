//! # Docker data caching layer
//!
//! A unified data layer that sits between the GUI controllers and the
//! Docker Engine. It provides:
//!
//! * [`RequestCoordinator`] — single-flight request deduplication plus a
//!   TTL in-memory cache. Concurrent callers with the same key share one
//!   Docker request; a result stays cached for its TTL.
//! * [`ImageMetadataCache`] — long-lived architecture/os/variant metadata
//!   keyed by content-addressed image ID.
//! * [`VolumeUsageCache`] — short-TTL volume size/ref-count with an
//!   explicit "unknown" state (never fabricates `0 B`).
//! * [`PersistentCache`] — SQLite-backed snapshot cache isolated by
//!   Docker endpoint, with write-behind flushing.
//! * [`DockerEventMonitor`] — a `/events` watcher with debouncing and
//!   exponential-backoff reconnect.
//! * [`PreviewSessionPool`] — a pool of reusable helper sessions plus
//!   per-directory TTL caching for volume file browsing.
//!
//! Everything in this module is pure Rust with no Qt dependency; GUI
//! bridges use these types directly.

mod coordinator;
mod events;
mod image_metadata;
mod persistent;
mod session_pool;
mod volume_usage;

pub use coordinator::{FlightOutcome, RequestCoordinator, TtlCache, TtlEntry};
pub use events::{
    ChangeKind, ChangeNotification, DefaultEventClassifier, DockerEventMonitor,
    DockerEventMonitorConfig, EventClassifier, run_monitor,
};
pub use image_metadata::{CachedImageMetadata, ImageMetadataCache};
pub use persistent::{
    CACHE_SCHEMA_VERSION, EndpointKey, PersistentCache, PersistentCacheConfig, SENSITIVE_KEYS,
    default_cache_path,
};
pub use session_pool::{
    CachedDirectory, CachedPreviewSession, PreviewSessionPool, PreviewSessionPoolConfig,
    PreviewSessionState,
};
pub use volume_usage::{CachedVolumeUsage, VolumeUsageCache, VolumeUsageSource};
