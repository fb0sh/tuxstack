//! Single-flight request deduplication and a TTL in-memory cache.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use futures_util::future::Shared;
use tokio::sync::Mutex;

/// An in-memory entry with an optional expiry.
#[derive(Debug, Clone)]
pub struct TtlEntry<V> {
    pub value: V,
    pub inserted_at: Instant,
    pub expires_at: Option<Instant>,
}

impl<V> TtlEntry<V> {
    pub fn fresh(value: V, ttl: Option<Duration>) -> Self {
        let inserted_at = Instant::now();
        Self {
            value,
            inserted_at,
            expires_at: ttl.map(|ttl| inserted_at + ttl),
        }
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.expires_at.map(|at| now >= at).unwrap_or(false)
    }
}

/// A simple TTL-backed map. Not a full LRU; callers prune on access.
pub struct TtlCache<K, V> {
    inner: HashMap<K, TtlEntry<V>>,
}

impl<K, V> Default for TtlCache<K, V> {
    fn default() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }
}

impl<K, V> TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&mut self, key: &K, ttl: Option<Duration>) -> Option<V> {
        let now = Instant::now();
        let expired = self
            .inner
            .get(key)
            .map(|entry| match ttl {
                Some(ttl) => now.saturating_duration_since(entry.inserted_at) >= ttl,
                None => false,
            })
            .unwrap_or(false);
        if expired {
            self.inner.remove(key);
            return None;
        }
        self.inner.get(key).map(|entry| entry.value.clone())
    }

    pub fn insert(&mut self, key: K, value: V, ttl: Option<Duration>) {
        self.inner.insert(key, TtlEntry::fresh(value, ttl));
    }

    pub fn remove(&mut self, key: &K) {
        self.inner.remove(key);
    }

    pub fn contains(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Prune entries older than `ttl`; returns the number removed.
    pub fn prune_expired(&mut self, ttl: Option<Duration>) -> usize {
        let now = Instant::now();
        let before = self.inner.len();
        let Some(ttl) = ttl else {
            return 0;
        };
        self.inner
            .retain(|_, entry| now.saturating_duration_since(entry.inserted_at) < ttl);
        before - self.inner.len()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

/// Result of a single-flight invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightOutcome {
    /// This call started and completed the shared future.
    Leader,
    /// This call waited on an in-flight future started by another caller.
    Joined,
}

/// Single-flight deduplication keyed by `K`.
///
/// Concurrent calls for the same key share one underlying future. Only the
/// first caller owns the future; the rest await a broadcast handle. When the
/// future completes the in-flight entry is removed so a later retry can run
/// again. Cancelling one waiter does not cancel the shared future, and the
/// shared future keeps running as long as any waiter (or the leader) holds a
/// handle — exactly the semantics required by the caching spec.
pub struct RequestCoordinator<K, V> {
    in_flight: Arc<Mutex<HashMap<K, Shared<BoxFuture<V>>>>>,
}

impl<K, V> Clone for RequestCoordinator<K, V> {
    fn clone(&self) -> Self {
        Self {
            in_flight: self.in_flight.clone(),
        }
    }
}

impl<K, V> Default for RequestCoordinator<K, V> {
    fn default() -> Self {
        Self {
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<K, V> RequestCoordinator<K, V>
where
    K: Eq + Hash + Clone + fmt::Debug,
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `produce` for `key` exactly once across concurrent callers.
    ///
    /// The first caller becomes the leader and runs `produce`; concurrent
    /// callers with the same key await the same result. When `produce`
    /// finishes (success or error), the in-flight entry is dropped so a
    /// subsequent call starts a fresh request.
    pub async fn run_once<F, Fut>(&self, key: K, produce: F) -> (FlightOutcome, V)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = V> + Send + 'static,
        V: Clone + Send + 'static,
    {
        let mut in_flight = self.in_flight.lock().await;

        if let Some(existing) = in_flight.get(&key) {
            let handle = existing.clone();
            drop(in_flight);
            let value = handle.await;
            return (FlightOutcome::Joined, value);
        }

        // Leader: build the shared future and store it before polling so a
        // concurrent caller can join instead of duplicating the request.
        let future: BoxFuture<V> = Box::pin(produce());
        let shared: Shared<BoxFuture<V>> = future.shared();
        in_flight.insert(key.clone(), shared.clone());
        drop(in_flight);

        let _guard = InFlightGuard {
            in_flight: self.in_flight.clone(),
            key: key.clone(),
        };
        let value = shared.clone().await;
        (FlightOutcome::Leader, value)
    }

    /// Drop any in-flight entry for `key`. Waiter handles already obtained
    /// continue to resolve; a new call after this starts a fresh request.
    pub async fn forget(&self, key: &K) {
        self.in_flight.lock().await.remove(key);
    }

    pub async fn in_flight_count(&self) -> usize {
        self.in_flight.lock().await.len()
    }

    pub async fn clear(&self) {
        self.in_flight.lock().await.clear();
    }
}

type BoxFuture<V> = Pin<Box<dyn Future<Output = V> + Send + 'static>>;

/// Removes the in-flight entry when the leader finishes — including when the
/// leader task is cancelled. The shared future itself keeps running for any
/// waiter that already holds a handle.
struct InFlightGuard<K, V>
where
    K: Eq + Hash,
{
    in_flight: Arc<Mutex<HashMap<K, Shared<BoxFuture<V>>>>>,
    key: K,
}

impl<K, V> Drop for InFlightGuard<K, V>
where
    K: Eq + Hash,
{
    fn drop(&mut self) {
        let mut in_flight = match self.in_flight.try_lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        in_flight.remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn ten_same_inspect_run_producer_once() {
        let coordinator = RequestCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let coordinator = coordinator.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                let (_, value) = coordinator
                    .run_once("image:abc", {
                        let calls = calls.clone();
                        || async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            "detail".to_string()
                        }
                    })
                    .await;
                value
            }));
        }

        let mut values = Vec::new();
        for handle in handles {
            values.push(handle.await.unwrap());
        }
        assert!(values.iter().all(|v| v == "detail"));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "producer must run once");
        assert_eq!(coordinator.in_flight_count().await, 0);
    }

    #[tokio::test]
    async fn later_callers_join_in_flight_request() {
        let coordinator = RequestCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let leader = tokio::spawn({
            let coordinator = coordinator.clone();
            let calls = calls.clone();
            async move {
                coordinator
                    .run_once("k", {
                        move || async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(40)).await;
                            42
                        }
                    })
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(5)).await;

        let (outcome, value) = coordinator
            .run_once("k", || async {
                unreachable!("producer must not run twice")
            })
            .await;
        assert_eq!(outcome, FlightOutcome::Joined);
        assert_eq!(value, 42);
        assert_eq!(leader.await.unwrap().1, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn different_keys_run_concurrently() {
        let coordinator = RequestCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let a = tokio::spawn({
            let coordinator = coordinator.clone();
            let calls = calls.clone();
            async move {
                coordinator
                    .run_once("a", {
                        move || async move {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            calls.fetch_add(1, Ordering::SeqCst);
                            "a"
                        }
                    })
                    .await
            }
        });

        let (_, value_b) = coordinator
            .run_once("b", {
                let calls = calls.clone();
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    "b"
                }
            })
            .await;
        assert_eq!(value_b, "b");
        assert_eq!(a.await.unwrap().1, "a");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        coordinator.clear().await;
    }

    #[tokio::test]
    async fn failure_does_not_poison_coordinator() {
        let coordinator = RequestCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let (_, first) = coordinator
            .run_once("k", {
                let calls = calls.clone();
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Result::<String, String>::Err("boom".into())
                }
            })
            .await;
        assert!(first.is_err());
        assert_eq!(coordinator.in_flight_count().await, 0);

        let (_, second) = coordinator
            .run_once("k", {
                let calls = calls.clone();
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Result::<String, String>::Ok("ok".into())
                }
            })
            .await;
        assert!(second.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelling_one_waiter_does_not_cancel_others() {
        let coordinator = RequestCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let slow = tokio::spawn({
            let coordinator = coordinator.clone();
            let calls = calls.clone();
            async move {
                coordinator
                    .run_once("slow", {
                        move || async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(80)).await;
                            "done"
                        }
                    })
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(5)).await;

        let waiter = tokio::spawn({
            let coordinator = coordinator.clone();
            async move {
                coordinator
                    .run_once("slow", || async {
                        unreachable!("producer must not run again");
                    })
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        waiter.abort();

        assert_eq!(slow.await.unwrap().1, "done");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        coordinator.clear().await;
    }

    #[tokio::test]
    async fn ttl_cache_expires_and_prunes() {
        let mut cache = TtlCache::<String, i32>::new();
        let ttl = Duration::from_millis(30);
        cache.insert("a".into(), 1, Some(ttl));
        assert_eq!(cache.get(&"a".into(), Some(ttl)), Some(1));

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(cache.get(&"a".into(), Some(ttl)), None);
        assert_eq!(cache.prune_expired(Some(ttl)), 0);

        cache.insert("b".into(), 2, Some(ttl));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(cache.prune_expired(Some(ttl)), 1);
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn ttl_cache_no_ttl_keeps_forever() {
        let mut cache = TtlCache::<String, i32>::new();
        cache.insert("a".into(), 1, None);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(cache.get(&"a".into(), None), Some(1));
        cache.remove(&"a".into());
        assert!(cache.is_empty());
    }
}
