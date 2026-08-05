//! Global Docker event monitor.
//!
//! Watches the Docker events stream and turns bursts of events into
//! debounced, categorized invalidation notifications so the GUI can refresh
//! exactly the affected models. Reconnects with exponential backoff and
//! jitter after failures, and never clears caches on reconnect.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::models::DockerEvent;
use crate::streams::events::{EventStream, EventStreamResult};

/// Which resource class a Docker event affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Images,
    Containers,
    Volumes,
    Networks,
    Daemon,
}

/// A container event retained in a debounced change batch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerChange {
    /// Docker actor ID. Empty only when the daemon omitted the actor ID.
    pub actor_id: String,
    /// Docker container event action.
    pub action: String,
}

/// Maximum number of distinct container changes retained in one batch.
const MAX_CONTAINER_CHANGES: usize = 256;

/// A debounced batch of changes ready for the consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeNotification {
    /// Distinct resource classes touched by the batch (order-stable).
    pub kinds: Vec<ChangeKind>,
    /// Distinct container changes in first-seen order.
    pub container_changes: Vec<ContainerChange>,
    /// Whether additional distinct container changes exceeded the batch cap.
    pub container_changes_truncated: bool,
    /// Number of raw events coalesced into this notification.
    pub burst: usize,
    /// Whether the stream had to reconnect before this batch.
    pub reconnected: bool,
}

struct PendingBatch {
    start: tokio::time::Instant,
    kinds: HashSet<ChangeKind>,
    container_changes: Vec<ContainerChange>,
    seen_container_changes: HashSet<ContainerChange>,
    container_changes_truncated: bool,
    burst: usize,
}

impl PendingBatch {
    fn new(kind: ChangeKind, event: &DockerEvent) -> Self {
        let mut batch = Self {
            start: tokio::time::Instant::now(),
            kinds: HashSet::new(),
            container_changes: Vec::new(),
            seen_container_changes: HashSet::new(),
            container_changes_truncated: false,
            burst: 0,
        };
        batch.add(kind, event);
        batch
    }

    fn add(&mut self, kind: ChangeKind, event: &DockerEvent) {
        self.kinds.insert(kind);
        self.burst += 1;
        if kind != ChangeKind::Containers {
            return;
        }

        let change = ContainerChange {
            actor_id: event.actor_id.clone().unwrap_or_default(),
            action: event.action.clone(),
        };
        if self.seen_container_changes.contains(&change) {
            return;
        }
        if self.container_changes.len() >= MAX_CONTAINER_CHANGES {
            self.container_changes_truncated = true;
            return;
        }
        self.seen_container_changes.insert(change.clone());
        self.container_changes.push(change);
    }
}

/// Classifies a raw Docker event into a [`ChangeKind`].
pub trait EventClassifier: Send + Sync + 'static {
    fn classify(&self, event: &DockerEvent) -> Option<ChangeKind>;
}

/// Classifier for the four resource types TuxStack manages plus daemon
/// identity changes.
#[derive(Debug, Clone, Default)]
pub struct DefaultEventClassifier;

impl EventClassifier for DefaultEventClassifier {
    fn classify(&self, event: &DockerEvent) -> Option<ChangeKind> {
        let kind = match event.event_type.as_str() {
            "image" => ChangeKind::Images,
            "container" => ChangeKind::Containers,
            "volume" => ChangeKind::Volumes,
            "network" => ChangeKind::Networks,
            "daemon" => ChangeKind::Daemon,
            _ => return None,
        };
        Some(kind)
    }
}

/// Configuration for the monitor.
#[derive(Debug, Clone)]
pub struct DockerEventMonitorConfig {
    /// Debounce window: events arriving within this window coalesce.
    pub debounce: Duration,
    /// Initial reconnect delay.
    pub backoff_initial: Duration,
    /// Maximum reconnect delay.
    pub backoff_max: Duration,
    /// Jitter added to each backoff delay (fraction of the delay).
    pub jitter: f64,
}

impl Default for DockerEventMonitorConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(250),
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(30),
            jitter: 0.3,
        }
    }
}

/// Watches `/events` and publishes debounced [`ChangeNotification`]s.
///
/// Spawns a background task on the Tokio runtime; callers receive
/// notifications through [`Self::subscribe`]. Reconnects use exponential
/// backoff with jitter and never clear caches. `shutdown` stops the loop.
#[derive(Clone)]
pub struct DockerEventMonitor {
    tx: watch::Sender<Option<ChangeNotification>>,
    token: CancellationToken,
    client: Arc<std::sync::Mutex<Option<Arc<DockerClient>>>>,
}

impl DockerEventMonitor {
    /// Create a monitor (does not start watching).
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(None);
        Self {
            tx,
            token: CancellationToken::new(),
            client: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Re-point the monitor at a connected client. Safe to call after a
    /// reconnect; the next `start` uses the new client.
    pub fn rebind(&mut self, client: Arc<DockerClient>) {
        self.rebind_client(client);
    }

    /// Clone-friendly variant of [`Self::rebind`] for shared clones.
    pub fn rebind_client(&self, client: Arc<DockerClient>) {
        if let Ok(mut inner) = self.client.lock() {
            *inner = Some(client);
        }
    }

    pub fn client(&self) -> Option<Arc<DockerClient>> {
        self.client.lock().ok().and_then(|c| c.clone())
    }

    /// Subscribe to debounced change notifications.
    pub fn subscribe(&self) -> watch::Receiver<Option<ChangeNotification>> {
        self.tx.subscribe()
    }

    /// Start the watch loop. Requires a client to be bound via `rebind`.
    /// Returns immediately; the loop runs until `shutdown` is called.
    pub fn start(&self) -> EventStreamResult
    where
        Self: Sized,
    {
        let Some(client) = self.client() else {
            // No client yet: return an empty stream so callers can select.
            return Box::pin(futures_util::stream::empty());
        };
        let stream = EventStream::new(client);
        stream.watch_events(self.token.clone())
    }

    /// Stop the watch loop.
    pub fn shutdown(&self) {
        self.token.cancel();
    }
}

impl Default for DockerEventMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the debounced watch loop. Consumes the stream, coalesces events,
/// and publishes [`ChangeNotification`]s. Reconnects with exponential
/// backoff on errors or stream end.
pub async fn run_monitor<C: EventClassifier>(
    monitor: &DockerEventMonitor,
    mut stream: EventStreamResult,
    classifier: C,
) {
    tracing::debug!("Docker event monitor loop started");
    let debounce = Duration::from_millis(250);
    let mut pending: Option<PendingBatch> = None;
    let mut reconnected = false;
    // Exponential backoff across reconnect attempts: 1s -> 30s (+ jitter).
    let mut backoff = Duration::from_secs(1);

    loop {
        tokio::select! {
            _ = monitor.token.cancelled() => break,
            event = stream.next() => {
                match event {
                    Some(Ok(ev)) => {
                        tracing::debug!(
                            event_type = %ev.event_type,
                            action = %ev.action,
                            "raw docker event received by monitor"
                        );
                        let Some(kind) = classifier.classify(&ev) else {
                            continue;
                        };
                        tracing::debug!(kind = ?kind, action = %ev.action, "Docker event received");
                        if pending
                            .as_ref()
                            .is_some_and(|batch| batch.start.elapsed() >= debounce)
                        {
                            // Window closed while we were away: flush and
                            // open a new one without losing this event.
                            flush(&monitor.tx, &mut pending, reconnected);
                            reconnected = false;
                        }
                        match &mut pending {
                            Some(batch) => batch.add(kind, &ev),
                            None => pending = Some(PendingBatch::new(kind, &ev)),
                        }
                        // A successful event resets the reconnect backoff.
                        backoff = Duration::from_secs(1);
                    }
                    Some(Err(error)) => {
                        tracing::debug!(%error, "Docker event stream error");
                        flush(&monitor.tx, &mut pending, reconnected);
                        reconnected = true;
                        // Backoff reconnect (never clears caches); jittered,
                        // growing 1s -> 30s. Cancel stops the loop.
                        if !wait_backoff(&monitor.token, backoff).await {
                            break;
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        if let Some(client) = monitor.client() {
                            let stream_service = EventStream::new(client);
                            stream = stream_service.watch_events(monitor.token.clone());
                        } else {
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("Docker event stream ended");
                        flush(&monitor.tx, &mut pending, reconnected);
                        reconnected = true;
                        // Backoff reconnect (never clears caches); jittered,
                        // growing 1s -> 30s. Cancel stops the loop.
                        if !wait_backoff(&monitor.token, backoff).await {
                            break;
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        if let Some(client) = monitor.client() {
                            let stream_service = EventStream::new(client);
                            stream = stream_service.watch_events(monitor.token.clone());
                        } else {
                            break;
                        }
                    }
                }
            }
            flushed = debounce_tick(&mut pending, &monitor.tx, debounce, reconnected) => {
                if flushed {
                    reconnected = false;
                }
            }
        }
    }
    flush(&monitor.tx, &mut pending, reconnected);
}

/// Debounce timer select arm: sleeps until the debounce window closes and
/// flushes the pending batch.
async fn debounce_tick(
    pending: &mut Option<PendingBatch>,
    tx: &watch::Sender<Option<ChangeNotification>>,
    debounce: Duration,
    reconnected: bool,
) -> bool {
    let Some(batch) = pending.as_ref() else {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        return false;
    };
    let remaining = debounce.saturating_sub(batch.start.elapsed());
    tokio::time::sleep(remaining).await;
    let mut batch = pending.take();
    flush(tx, &mut batch, reconnected);
    true
}

/// Wait for the reconnect backoff; returns false if cancelled.
async fn wait_backoff(token: &CancellationToken, delay: Duration) -> bool {
    let jittered = jitter(delay);
    tokio::select! {
        _ = token.cancelled() => false,
        _ = tokio::time::sleep(jittered) => true,
    }
}

/// Publish a pending batch (if any) as a [`ChangeNotification`].
fn flush(
    tx: &watch::Sender<Option<ChangeNotification>>,
    pending: &mut Option<PendingBatch>,
    reconnected: bool,
) {
    let Some(batch) = pending.take() else {
        return;
    };
    let mut kinds: Vec<ChangeKind> = batch.kinds.into_iter().collect();
    // Deterministic order: Images, Containers, Volumes, Networks, Daemon.
    let order = [
        ChangeKind::Images,
        ChangeKind::Containers,
        ChangeKind::Volumes,
        ChangeKind::Networks,
        ChangeKind::Daemon,
    ];
    kinds.sort_by_key(|k| order.iter().position(|c| c == k).unwrap_or(usize::MAX));
    let _ = tx.send(Some(ChangeNotification {
        kinds,
        container_changes: batch.container_changes,
        container_changes_truncated: batch.container_changes_truncated,
        burst: batch.burst,
        reconnected,
    }));
}

/// Add ±jitter fraction to a delay.
fn jitter(delay: Duration) -> Duration {
    let frac = 0.3;
    let nanos = delay.as_nanos() as f64;
    let spread = nanos * frac;
    let jitter_ns = (spread * fastrand_signed()).max(0.0) as u64;
    delay.saturating_add(Duration::from_nanos(jitter_ns))
}

fn fastrand_signed() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let x = (nanos as f64 / 1_000_000_000.0).fract();
    (x * 2.0) - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: &str) -> DockerEvent {
        container_event(event_type, "abc", "create")
    }

    fn container_event(event_type: &str, actor_id: &str, action: &str) -> DockerEvent {
        DockerEvent {
            event_type: event_type.to_string(),
            action: action.to_string(),
            actor_id: Some(actor_id.to_string()),
            actor_attributes: vec![],
            time: None,
        }
    }

    #[test]
    fn default_classifier_covers_all_types() {
        let classifier = DefaultEventClassifier;
        assert_eq!(
            classifier.classify(&event("image")),
            Some(ChangeKind::Images)
        );
        assert_eq!(
            classifier.classify(&event("container")),
            Some(ChangeKind::Containers)
        );
        assert_eq!(
            classifier.classify(&event("volume")),
            Some(ChangeKind::Volumes)
        );
        assert_eq!(
            classifier.classify(&event("network")),
            Some(ChangeKind::Networks)
        );
        assert_eq!(
            classifier.classify(&event("daemon")),
            Some(ChangeKind::Daemon)
        );
        assert_eq!(classifier.classify(&event("whatever")), None);
    }

    #[test]
    fn flush_orders_kinds_and_retains_container_details() {
        let (tx, _rx) = watch::channel(None);
        let first = container_event("container", "first", "start");
        let mut pending = Some(PendingBatch::new(ChangeKind::Containers, &first));
        let batch = pending.as_mut().unwrap();
        batch.add(ChangeKind::Images, &event("image"));
        batch.add(ChangeKind::Networks, &event("network"));
        batch.add(
            ChangeKind::Containers,
            &container_event("container", "second", "die"),
        );

        flush(&tx, &mut pending, true);
        let value = tx.borrow().clone().expect("sent");
        assert_eq!(
            value.kinds,
            vec![
                ChangeKind::Images,
                ChangeKind::Containers,
                ChangeKind::Networks
            ]
        );
        assert_eq!(
            value.container_changes,
            vec![
                ContainerChange {
                    actor_id: "first".to_string(),
                    action: "start".to_string(),
                },
                ContainerChange {
                    actor_id: "second".to_string(),
                    action: "die".to_string(),
                },
            ]
        );
        assert!(!value.container_changes_truncated);
        assert_eq!(value.burst, 4);
        assert!(value.reconnected);
    }

    #[test]
    fn container_changes_dedupe_by_id_and_action_in_first_seen_order() {
        let first = container_event("container", "same", "start");
        let mut batch = PendingBatch::new(ChangeKind::Containers, &first);
        batch.add(ChangeKind::Containers, &first);
        batch.add(
            ChangeKind::Containers,
            &container_event("container", "same", "die"),
        );
        batch.add(
            ChangeKind::Containers,
            &container_event("container", "other", "start"),
        );

        assert_eq!(batch.burst, 4);
        assert_eq!(
            batch.container_changes,
            vec![
                ContainerChange {
                    actor_id: "same".to_string(),
                    action: "start".to_string(),
                },
                ContainerChange {
                    actor_id: "same".to_string(),
                    action: "die".to_string(),
                },
                ContainerChange {
                    actor_id: "other".to_string(),
                    action: "start".to_string(),
                },
            ]
        );
    }

    #[test]
    fn container_change_batch_is_capped_without_affecting_burst() {
        let first = container_event("container", "container-0", "start");
        let mut batch = PendingBatch::new(ChangeKind::Containers, &first);
        for index in 1..=MAX_CONTAINER_CHANGES {
            batch.add(
                ChangeKind::Containers,
                &container_event("container", &format!("container-{index}"), "start"),
            );
        }

        assert_eq!(batch.container_changes.len(), MAX_CONTAINER_CHANGES);
        assert_eq!(batch.container_changes[0].actor_id, "container-0");
        assert_eq!(
            batch.container_changes[MAX_CONTAINER_CHANGES - 1].actor_id,
            format!("container-{}", MAX_CONTAINER_CHANGES - 1)
        );
        assert!(batch.container_changes_truncated);
        assert_eq!(batch.burst, MAX_CONTAINER_CHANGES + 1);
    }

    #[test]
    fn non_container_change_keeps_container_details_empty() {
        let batch = PendingBatch::new(ChangeKind::Images, &event("image"));
        assert!(batch.container_changes.is_empty());
        assert!(!batch.container_changes_truncated);
    }

    #[test]
    fn empty_flush_sends_nothing() {
        let (tx, rx) = watch::channel(None);
        let mut pending = None;
        flush(&tx, &mut pending, false);
        assert!(rx.borrow().is_none());
    }

    #[test]
    fn jitter_never_reduces_to_zero() {
        for _ in 0..50 {
            let j = jitter(Duration::from_secs(1));
            assert!(j >= Duration::from_secs(1));
            assert!(j <= Duration::from_millis(1600));
        }
    }
}
