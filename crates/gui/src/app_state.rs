//! Shared application state: the services registry, page status
//! machine, and pure (testable) page-state logic.
//!
//! The QML-facing bridge objects are thin; all state transitions live
//! here so they can be unit-tested without Qt.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tuxstack_docker_core::{ContainerSummary, DockerError, DockerServices};

use crate::error::AppError;
use crate::settings::GuiSettings;

/// Registry of the shared Docker services, set once the app connects.
static SERVICES: Mutex<Option<Arc<DockerServices>>> = Mutex::new(None);

/// Registry of GUI settings (set at startup).
static SETTINGS: OnceLock<GuiSettings> = OnceLock::new();

/// Store the shared services after a successful connection.
pub fn set_services(services: DockerServices) {
    *SERVICES.lock().expect("services lock") = Some(Arc::new(services));
}

/// Clear a previous connection before starting a new connection attempt.
pub fn clear_services() {
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

/// Numeric page status exposed to QML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStatus {
    Idle = 0,
    Loading = 1,
    Ready = 2,
    Empty = 3,
    Error = 4,
    DockerUnavailable = 5,
}

impl PageStatus {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// A single row of the container list model.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerRow {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    pub created_at: String,
    pub cpu_percent: f64,
    pub memory_usage: u64,
    pub memory_limit: u64,
}

impl ContainerRow {
    pub fn running(&self) -> bool {
        matches!(self.state.as_str(), "running" | "paused" | "restarting")
    }
}

/// Pure state machine for the containers page.
#[derive(Debug, Clone)]
pub struct ContainerPageState {
    pub rows: Vec<ContainerRow>,
    pub busy: HashMap<String, String>,
    pub generation: u64,
    pub status: PageStatus,
    pub status_text: String,
}

impl Default for ContainerPageState {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            busy: HashMap::new(),
            generation: 0,
            status: PageStatus::Idle,
            status_text: String::new(),
        }
    }
}

impl ContainerPageState {
    /// Begin a refresh; returns the generation token that results must
    /// carry so stale responses cannot overwrite newer ones.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation += 1;
        self.status = PageStatus::Loading;
        self.generation
    }

    /// Apply a successful list result if its generation is current.
    pub fn apply_list(&mut self, generation: u64, summaries: &[ContainerSummary]) -> bool {
        if generation != self.generation {
            return false;
        }
        self.rows = summaries
            .iter()
            .map(|c| ContainerRow {
                id: c.id.clone(),
                short_id: c.short_id.clone(),
                name: c.name.clone(),
                image: c.image.clone(),
                state: c.state.as_str().to_string(),
                status: c.status.clone(),
                ports: c
                    .ports
                    .iter()
                    .map(|p| p.display())
                    .collect::<Vec<_>>()
                    .join(", "),
                created_at: c.created_at.format("%Y-%m-%d %H:%M").to_string(),
                cpu_percent: 0.0,
                memory_usage: 0,
                memory_limit: 0,
            })
            .collect();
        self.status = if self.rows.is_empty() {
            PageStatus::Empty
        } else {
            PageStatus::Ready
        };
        true
    }

    /// Apply a list failure if its generation is current.
    pub fn apply_list_error(&mut self, generation: u64, error: &AppError) -> bool {
        if generation != self.generation {
            return false;
        }
        self.rows.clear();
        self.status = match error.kind() {
            "docker_unavailable" => PageStatus::DockerUnavailable,
            "permission_denied" => PageStatus::DockerUnavailable,
            _ => PageStatus::Error,
        };
        self.status_text = error.user_message();
        true
    }

    /// Apply one stats sample for a container (used by the list columns).
    pub fn apply_stats(&mut self, generation: u64, id: &str, cpu: f64, mem: u64, limit: u64) {
        if generation != self.generation {
            return;
        }
        if let Some(row) = self.rows.iter_mut().find(|r| r.id == id) {
            row.cpu_percent = cpu;
            row.memory_usage = mem;
            row.memory_limit = limit;
        }
    }

    /// Mark a container as busy with an operation label.
    pub fn mark_busy(&mut self, id: &str, operation: &str) {
        self.busy.insert(id.to_string(), operation.to_string());
    }

    /// Clear the busy flag; returns true if it was set.
    pub fn clear_busy(&mut self, id: &str) -> bool {
        self.busy.remove(id).is_some()
    }

    /// Whether the container is currently busy.
    pub fn is_busy(&self, id: &str) -> bool {
        self.busy.contains_key(id)
    }

    /// Whether the given operation is allowed while `current` is running.
    #[allow(dead_code)] // used by QML-side action gating via is_busy
    pub fn operation_allowed(&self, id: &str, _operation: &str) -> bool {
        !self.is_busy(id)
    }
}

/// A single row of the volume list model.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub scope: String,
    pub created_at: String,
}

/// Small wrapper for the services registry error mapping.
pub fn map_docker_error(err: &DockerError) -> AppError {
    AppError::from(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuxstack_docker_core::models::{ContainerState, PortBinding};

    fn summary(id: String, name: &str, state: ContainerState) -> ContainerSummary {
        ContainerSummary {
            id: id.to_string(),
            short_id: id[..id.len().min(12)].to_string(),
            name: name.to_string(),
            image: "busybox:latest".to_string(),
            image_id: String::new(),
            state,
            status: "Up".to_string(),
            created_at: chrono::Utc::now(),
            ports: vec![PortBinding {
                host_ip: None,
                host_port: Some(8080),
                container_port: 80,
                protocol: "tcp".to_string(),
            }],
            labels: Default::default(),
        }
    }

    #[test]
    fn loading_to_ready() {
        let mut s = ContainerPageState::default();
        let generation = s.begin_refresh();
        assert_eq!(s.status, PageStatus::Loading);

        let applied = s.apply_list(
            generation,
            &[summary("a".repeat(64), "web", ContainerState::Running)],
        );
        assert!(applied);
        assert_eq!(s.status, PageStatus::Ready);
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].name, "web");
        assert_eq!(s.rows[0].ports, "8080->80/tcp");
    }

    #[test]
    fn loading_to_empty() {
        let mut s = ContainerPageState::default();
        let generation = s.begin_refresh();
        assert!(s.apply_list(generation, &[]));
        assert_eq!(s.status, PageStatus::Empty);
    }

    #[test]
    fn loading_to_error() {
        let mut s = ContainerPageState::default();
        let generation = s.begin_refresh();
        let err = AppError::Docker("boom".into());
        assert!(s.apply_list_error(generation, &err));
        assert_eq!(s.status, PageStatus::Error);
        assert_eq!(s.status_text, "boom");
    }

    #[test]
    fn loading_to_docker_unavailable() {
        let mut s = ContainerPageState::default();
        let generation = s.begin_refresh();
        let err = AppError::DockerUnavailable("socket".into());
        assert!(s.apply_list_error(generation, &err));
        assert_eq!(s.status, PageStatus::DockerUnavailable);
    }

    #[test]
    fn stale_generation_is_ignored() {
        let mut s = ContainerPageState::default();
        let generation_1 = s.begin_refresh();
        let generation_2 = s.begin_refresh();

        // Old request finishes late and must be dropped.
        assert!(!s.apply_list(
            generation_1,
            &[summary("a".repeat(64), "old", ContainerState::Running)]
        ));
        // New request applies.
        assert!(s.apply_list(
            generation_2,
            &[summary("b".repeat(64), "new", ContainerState::Running)]
        ));
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].name, "new");
    }

    #[test]
    fn busy_state_blocks_operations() {
        let mut s = ContainerPageState::default();
        let id = "abc123";
        assert!(!s.is_busy(id));
        assert!(s.operation_allowed(id, "start"));
        s.mark_busy(id, "stopping");
        assert!(s.is_busy(id));
        assert!(!s.operation_allowed(id, "start"));
        assert!(s.clear_busy(id));
        assert!(!s.is_busy(id));
        assert!(!s.clear_busy(id));
    }

    #[test]
    fn stats_update_matching_row() {
        let mut s = ContainerPageState::default();
        let generation = s.begin_refresh();
        let id = "a".repeat(64);
        s.apply_list(
            generation,
            &[summary(id.clone(), "web", ContainerState::Running)],
        );
        s.apply_stats(generation, &id, 12.5, 1000, 2000);
        assert_eq!(s.rows[0].cpu_percent, 12.5);
        assert_eq!(s.rows[0].memory_usage, 1000);
        // stale generation must not update
        s.apply_stats(generation - 1, &id, 99.0, 9999, 9999);
        assert_eq!(s.rows[0].cpu_percent, 12.5);
    }
}
