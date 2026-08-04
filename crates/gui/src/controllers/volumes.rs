//! Pure state/controller logic for Docker volumes.
//!
//! Debouncing intentionally does not live here: QML owns the 150–250 ms
//! timer and calls `set_search_query` only after it fires. Filtering itself is
//! local, deterministic, and never starts Docker work.

use std::cmp::Ordering;
use std::collections::HashMap;

use tuxstack_docker_core::{
    DockerError, VolumeContainerReference, VolumeDetail, VolumeSummary, VolumeUsage,
};

use crate::models::volume_model::{VolumeDetailView, VolumeRow, VolumeSizeSummary};

/// List-level state. Detail and operation failures never replace this state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum VolumesListState {
    #[default]
    Idle = 0,
    Loading = 1,
    Ready = 2,
    Empty = 3,
    Error = 4,
    DockerUnavailable = 5,
    PermissionDenied = 6,
}

impl VolumesListState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::Error => "error",
            Self::DockerUnavailable => "docker_unavailable",
            Self::PermissionDenied => "permission_denied",
        }
    }
}

/// Detail-level state. A list refresh failure always leaves this at `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum VolumeDetailState {
    #[default]
    None = 0,
    Loading = 1,
    Ready = 2,
    Error = 3,
}

impl VolumeDetailState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

/// Per-volume operation exposed by the list model's `operation` role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum VolumeOperation {
    Removing = 0,
    Exporting = 1,
    Cloning = 2,
}

impl VolumeOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Removing => "removing",
            Self::Exporting => "exporting",
            Self::Cloning => "cloning",
        }
    }
}

/// Operations which do not belong exclusively to an existing row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GlobalVolumeOperation {
    Creating = 0,
    Pruning = 1,
}

/// Stable numeric values for the QML sort menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum VolumeSortMode {
    NameAscending = 0,
    NameDescending = 1,
    NewestFirst = 2,
    OldestFirst = 3,
    LargestFirst = 4,
    SmallestFirst = 5,
    MostContainers = 6,
    FewestContainers = 7,
    #[default]
    InUseFirst = 8,
    UnusedFirst = 9,
}

impl VolumeSortMode {
    #[cfg(test)]
    pub fn from_i32(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::NameAscending,
            1 => Self::NameDescending,
            2 => Self::NewestFirst,
            3 => Self::OldestFirst,
            4 => Self::LargestFirst,
            5 => Self::SmallestFirst,
            6 => Self::MostContainers,
            7 => Self::FewestContainers,
            8 => Self::InUseFirst,
            9 => Self::UnusedFirst,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NameAscending => "name_asc",
            Self::NameDescending => "name_desc",
            Self::NewestFirst => "newest",
            Self::OldestFirst => "oldest",
            Self::LargestFirst => "largest",
            Self::SmallestFirst => "smallest",
            Self::MostContainers => "most_containers",
            Self::FewestContainers => "fewest_containers",
            Self::InUseFirst => "in_use_first",
            Self::UnusedFirst => "unused_first",
        }
    }
}

/// Cancellation is a request/acknowledgement handshake. Requesting does not
/// clear busy state; the bridge clears it only after the async task exits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum CancellationState {
    #[default]
    Idle = 0,
    Cancellable = 1,
    CancellationRequested = 2,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CancellableVolumeTask {
    pub active: bool,
    pub volume_name: String,
    pub generation: u64,
    pub cancellation: CancellationState,
}

/// Pure, Qt-free state machine backing the future CXX-Qt bridge.
#[derive(Debug, Clone)]
pub struct VolumesState {
    pub source_rows: Vec<VolumeRow>,
    pub visible_rows: Vec<VolumeRow>,
    pub search_query: String,
    pub sort_mode: VolumeSortMode,
    pub selected_volume_name: String,
    pub detail: Option<VolumeDetailView>,
    pub initialized: bool,
    pub list_state: VolumesListState,
    pub list_error_kind: String,
    pub list_error_message: String,
    pub detail_state: VolumeDetailState,
    pub detail_error_kind: String,
    pub detail_error_message: String,
    pub refresh_generation: u64,
    pub detail_generation: u64,
    pub operation_generation: u64,
    pub global_generation: u64,
    pub operations: HashMap<String, VolumeOperation>,
    pub operation_generations: HashMap<String, u64>,
    pub global_operation: Option<GlobalVolumeOperation>,
    pub export_task: CancellableVolumeTask,
    pub clone_task: CancellableVolumeTask,
    pub prune_cancellation: CancellationState,
    pub operation_error_kind: String,
    pub operation_error_message: String,
    /// Daemon-returned create/clone name preferred once on the next refresh.
    pub preferred_volume_name: String,
    /// Selection hidden by a search, restored when the search is cleared.
    pub selection_before_filter: String,
}

impl Default for VolumesState {
    fn default() -> Self {
        Self {
            source_rows: vec![],
            visible_rows: vec![],
            search_query: String::new(),
            sort_mode: VolumeSortMode::default(),
            selected_volume_name: String::new(),
            detail: None,
            initialized: false,
            list_state: VolumesListState::Idle,
            list_error_kind: String::new(),
            list_error_message: String::new(),
            detail_state: VolumeDetailState::None,
            detail_error_kind: String::new(),
            detail_error_message: String::new(),
            refresh_generation: 0,
            detail_generation: 0,
            operation_generation: 0,
            global_generation: 0,
            operations: HashMap::new(),
            operation_generations: HashMap::new(),
            global_operation: None,
            export_task: CancellableVolumeTask::default(),
            clone_task: CancellableVolumeTask::default(),
            prune_cancellation: CancellationState::Idle,
            operation_error_kind: String::new(),
            operation_error_message: String::new(),
            preferred_volume_name: String::new(),
            selection_before_filter: String::new(),
        }
    }
}

impl VolumesState {
    /// Returns true exactly once. The bridge should call `refresh` only when
    /// this returns true, making page re-entry reuse existing state.
    pub fn initialize(&mut self) -> bool {
        if self.initialized {
            return false;
        }
        self.initialized = true;
        true
    }

    /// Start a refresh and invalidate all in-flight detail requests.
    pub fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.invalidate_detail(VolumeDetailState::None);
        self.list_state = VolumesListState::Loading;
        self.list_error_kind.clear();
        self.list_error_message.clear();
        self.refresh_generation
    }

    /// Apply the combined list/container/disk-usage result.
    pub fn apply_list(&mut self, generation: u64, summaries: &[VolumeSummary]) -> bool {
        if generation != self.refresh_generation {
            return false;
        }

        let previous = self.selected_volume_name.clone();
        // Stage A returns summaries without container association. Carry over
        // the last known in-use state so the In Use / Unused grouping does not
        // flash to all-Unused on every refresh before Stage B patches usage.
        let known_in_use: HashMap<&str, bool> = self
            .source_rows
            .iter()
            .map(|row| (row.volume_name.as_str(), row.in_use))
            .collect();
        self.source_rows = summaries
            .iter()
            .map(VolumeRow::from)
            .map(|mut row| {
                if let Some(&in_use) = known_in_use.get(row.volume_name.as_str()) {
                    row.in_use = in_use;
                    row.section = if in_use { "in_use" } else { "unused" }.to_string();
                }
                row
            })
            .collect();
        self.rebuild_visible();
        self.list_state = if self.source_rows.is_empty() {
            VolumesListState::Empty
        } else {
            VolumesListState::Ready
        };
        self.list_error_kind.clear();
        self.list_error_message.clear();

        let preferred = std::mem::take(&mut self.preferred_volume_name);
        let desired = if !preferred.is_empty() && self.source_row_exists(&preferred) {
            Some(preferred)
        } else if self.visible_row_exists(&previous) {
            Some(previous)
        } else if self.search_query.is_empty()
            && !self.selection_before_filter.is_empty()
            && self.source_row_exists(&self.selection_before_filter)
        {
            Some(self.selection_before_filter.clone())
        } else {
            self.visible_rows.first().map(|row| row.volume_name.clone())
        };

        if let Some(name) = desired {
            self.set_selection_without_inspect(name);
        } else {
            self.clear_selection();
        }
        self.sync_dynamic_roles();
        true
    }

    pub fn apply_list_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        let friendly = friendly_volume_error(error, VolumeErrorContext::List);
        self.source_rows.clear();
        self.visible_rows.clear();
        self.clear_selection();
        self.list_state = friendly.list_state;
        self.list_error_kind = friendly.kind.to_string();
        self.list_error_message = friendly.message.to_string();
        true
    }

    /// Patch usage (size/ref-count) and container references for existing
    /// rows after the background enrichment completes. Never replaces the
    /// list or resets the selection; only values that changed are updated.
    pub fn patch_usage(
        &mut self,
        references: &HashMap<String, Vec<VolumeContainerReference>>,
        usage: &HashMap<String, VolumeUsage>,
    ) -> usize {
        let mut patched = 0usize;
        for row in &mut self.source_rows {
            let mut changed = false;
            if let Some(container_refs) = references.get(&row.volume_name) {
                let in_use = !container_refs.is_empty();
                if row.in_use != in_use || row.used_by_count != container_refs.len() {
                    row.in_use = in_use;
                    row.section = if in_use { "in_use" } else { "unused" }.to_string();
                    row.used_by_count = container_refs.len();
                    row.containers = container_refs.clone();
                    changed = true;
                }
            }
            if let Some(usage_entry) = usage.get(&row.volume_name) {
                let size_bytes = usage_entry.size_bytes;
                let size_text = size_bytes
                    .map(crate::models::volume_model::format_bytes)
                    .unwrap_or_else(|| "Unknown size".to_string());
                if row.size_bytes != size_bytes || row.size_text != size_text {
                    row.size_bytes = size_bytes;
                    row.size_known = size_bytes.is_some();
                    row.size_text = size_text;
                    changed = true;
                }
            }
            if changed {
                patched += 1;
            }
        }
        for row in &mut self.visible_rows {
            if let Some(container_refs) = references.get(&row.volume_name) {
                let in_use = !container_refs.is_empty();
                if row.in_use != in_use || row.used_by_count != container_refs.len() {
                    row.in_use = in_use;
                    row.section = if in_use { "in_use" } else { "unused" }.to_string();
                    row.used_by_count = container_refs.len();
                    row.containers = container_refs.clone();
                }
            }
            if let Some(usage_entry) = usage.get(&row.volume_name) {
                let size_bytes = usage_entry.size_bytes;
                let size_text = size_bytes
                    .map(crate::models::volume_model::format_bytes)
                    .unwrap_or_else(|| "Unknown size".to_string());
                if row.size_bytes != size_bytes || row.size_text != size_text {
                    row.size_bytes = size_bytes;
                    row.size_known = size_bytes.is_some();
                    row.size_text = size_text;
                }
            }
        }
        self.rebuild_visible();
        patched
    }

    /// Filtering is trim/case-insensitive and local. The returned generation,
    /// when present, must be used to inspect the automatically selected row.
    pub fn set_search_query(&mut self, query: &str) -> Option<u64> {
        let query = query.trim().to_string();
        let was_filtered = !self.search_query.is_empty();
        let will_be_filtered = !query.is_empty();
        if !was_filtered && will_be_filtered {
            self.selection_before_filter = self.selected_volume_name.clone();
        }

        let previous = self.selected_volume_name.clone();
        self.search_query = query;
        self.rebuild_visible();

        let desired = if !will_be_filtered
            && !self.selection_before_filter.is_empty()
            && self.source_row_exists(&self.selection_before_filter)
        {
            Some(std::mem::take(&mut self.selection_before_filter))
        } else if self.visible_row_exists(&previous) {
            Some(previous)
        } else {
            self.visible_rows.first().map(|row| row.volume_name.clone())
        };
        if !will_be_filtered {
            self.selection_before_filter.clear();
        }
        self.change_automatic_selection(desired)
    }

    pub fn set_sort_mode(&mut self, mode: VolumeSortMode) {
        self.sort_mode = mode;
        self.rebuild_visible();
        self.sync_dynamic_roles();
    }

    /// Explicit user selection. Inspect only the selected volume.
    pub fn select(&mut self, name: &str) -> Option<u64> {
        if !self.visible_row_exists(name) {
            return None;
        }
        if self.selected_volume_name == name && self.detail_state != VolumeDetailState::None {
            return None;
        }
        self.selected_volume_name = name.to_string();
        self.sync_dynamic_roles();
        Some(self.start_selected_inspect())
    }

    /// Start initial, refresh, or retry inspection of the selected row.
    pub fn begin_selected_inspect(&mut self) -> Option<u64> {
        if self.selected_volume_name.is_empty()
            || !self.visible_row_exists(&self.selected_volume_name)
        {
            return None;
        }
        Some(self.start_selected_inspect())
    }

    pub fn clear_selection(&mut self) {
        self.selected_volume_name.clear();
        self.invalidate_detail(VolumeDetailState::None);
        self.sync_dynamic_roles();
    }

    pub fn apply_detail(&mut self, generation: u64, detail: &VolumeDetail) -> bool {
        if generation != self.detail_generation
            || detail.summary.name != self.selected_volume_name
            || !self.source_row_exists(&detail.summary.name)
        {
            return false;
        }

        // Inspect can enrich usage/status fields. Keep future local searches and
        // summaries accurate without exposing the domain object to QML.
        if let Some(row) = self
            .source_rows
            .iter_mut()
            .find(|row| row.volume_name == detail.summary.name)
        {
            *row = VolumeRow::from(&detail.summary);
        }
        self.rebuild_visible();
        self.detail = Some(VolumeDetailView::from(detail));
        self.detail_state = VolumeDetailState::Ready;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
        self.sync_dynamic_roles();
        true
    }

    pub fn apply_detail_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.detail_generation || self.selected_volume_name.is_empty() {
            return false;
        }
        let friendly = friendly_volume_error(error, VolumeErrorContext::Detail);
        self.detail = None;
        self.detail_state = VolumeDetailState::Error;
        self.detail_error_kind = friendly.kind.to_string();
        self.detail_error_message = friendly.message.to_string();
        true
    }

    /// Remove a deleted row and select its next visible neighbour, then its
    /// previous neighbour. Returns the new selected-only inspect generation.
    pub fn remove_local(&mut self, name: &str) -> Option<u64> {
        let old_visible_index = self
            .visible_rows
            .iter()
            .position(|row| row.volume_name == name)
            .unwrap_or(0);
        self.source_rows.retain(|row| row.volume_name != name);
        self.operations.remove(name);
        self.operation_generations.remove(name);
        if self.selection_before_filter == name {
            self.selection_before_filter.clear();
        }
        self.rebuild_visible();

        let generation = if self.selected_volume_name == name {
            let neighbour = self
                .visible_rows
                .get(old_visible_index.min(self.visible_rows.len().saturating_sub(1)))
                .map(|row| row.volume_name.clone());
            if let Some(neighbour) = neighbour {
                self.selected_volume_name = neighbour;
                self.sync_dynamic_roles();
                Some(self.start_selected_inspect())
            } else {
                self.clear_selection();
                None
            }
        } else {
            self.sync_dynamic_roles();
            None
        };
        self.list_state = if self.source_rows.is_empty() {
            VolumesListState::Empty
        } else {
            VolumesListState::Ready
        };
        generation
    }

    /// Prefer a daemon-returned create/clone name on the next successful list.
    /// Clearing the filter ensures the preferred row can actually be selected.
    pub fn prefer_volume(&mut self, name: &str) {
        self.preferred_volume_name = name.to_string();
        self.search_query.clear();
        self.selection_before_filter.clear();
        self.rebuild_visible();
    }

    pub fn begin_create(&mut self) -> Option<u64> {
        self.begin_global_operation(GlobalVolumeOperation::Creating)
    }

    pub fn finish_create(&mut self, generation: u64, created_name: &str) -> bool {
        if !self.finish_global_operation(generation, GlobalVolumeOperation::Creating) {
            return false;
        }
        self.prefer_volume(created_name);
        true
    }

    pub fn begin_prune(&mut self) -> Option<u64> {
        let generation = self.begin_global_operation(GlobalVolumeOperation::Pruning)?;
        self.prune_cancellation = CancellationState::Cancellable;
        Some(generation)
    }

    pub fn request_cancel_prune(&mut self) -> bool {
        if self.global_operation != Some(GlobalVolumeOperation::Pruning)
            || self.prune_cancellation != CancellationState::Cancellable
        {
            return false;
        }
        self.prune_cancellation = CancellationState::CancellationRequested;
        true
    }

    pub fn finish_prune(&mut self, generation: u64) -> bool {
        if !self.finish_global_operation(generation, GlobalVolumeOperation::Pruning) {
            return false;
        }
        self.prune_cancellation = CancellationState::Idle;
        true
    }

    pub fn fail_global_operation(&mut self, generation: u64, error: &DockerError) -> bool {
        if self.global_operation.is_none() || generation != self.global_generation {
            return false;
        }
        if self.global_operation == Some(GlobalVolumeOperation::Pruning) {
            self.prune_cancellation = CancellationState::Idle;
        }
        self.global_operation = None;
        self.set_operation_error(error);
        true
    }

    pub fn begin_remove(&mut self, name: &str) -> Option<u64> {
        if !self.source_row_exists(name) {
            return None;
        }
        self.begin_volume_operation(name, VolumeOperation::Removing)
    }

    pub fn finish_remove(&mut self, generation: u64, name: &str) -> bool {
        self.finish_volume_operation(generation, name, VolumeOperation::Removing)
    }

    pub fn fail_volume_operation(
        &mut self,
        generation: u64,
        name: &str,
        operation: VolumeOperation,
        error: &DockerError,
    ) -> bool {
        if !self.volume_operation_matches(generation, name, operation) {
            return false;
        }
        self.operations.remove(name);
        self.operation_generations.remove(name);
        if operation == VolumeOperation::Exporting && task_matches(&self.export_task, generation) {
            self.export_task = CancellableVolumeTask::default();
        }
        if operation == VolumeOperation::Cloning && task_matches(&self.clone_task, generation) {
            self.clone_task = CancellableVolumeTask::default();
        }
        self.set_operation_error(error);
        self.sync_dynamic_roles();
        true
    }

    pub fn begin_export(&mut self, name: &str) -> Option<u64> {
        if self.export_task.active {
            return None;
        }
        let generation = self.begin_volume_operation(name, VolumeOperation::Exporting)?;
        self.export_task = CancellableVolumeTask {
            active: true,
            volume_name: name.to_string(),
            generation,
            cancellation: CancellationState::Cancellable,
        };
        Some(generation)
    }

    pub fn request_cancel_export(&mut self) -> bool {
        request_cancellation(&mut self.export_task)
    }

    pub fn finish_export(&mut self, generation: u64) -> bool {
        if !task_matches(&self.export_task, generation) {
            return false;
        }
        let name = self.export_task.volume_name.clone();
        if !self.finish_volume_operation(generation, &name, VolumeOperation::Exporting) {
            return false;
        }
        self.export_task = CancellableVolumeTask::default();
        true
    }

    pub fn fail_export(&mut self, generation: u64, error: &DockerError) -> bool {
        if !task_matches(&self.export_task, generation) {
            return false;
        }
        let name = self.export_task.volume_name.clone();
        if !self.finish_volume_operation(generation, &name, VolumeOperation::Exporting) {
            return false;
        }
        self.export_task = CancellableVolumeTask::default();
        self.set_operation_error(error);
        true
    }

    pub fn begin_clone(&mut self, source: &str) -> Option<u64> {
        if self.clone_task.active {
            return None;
        }
        let generation = self.begin_volume_operation(source, VolumeOperation::Cloning)?;
        self.clone_task = CancellableVolumeTask {
            active: true,
            volume_name: source.to_string(),
            generation,
            cancellation: CancellationState::Cancellable,
        };
        Some(generation)
    }

    pub fn request_cancel_clone(&mut self) -> bool {
        request_cancellation(&mut self.clone_task)
    }

    pub fn finish_clone(&mut self, generation: u64, created_name: Option<&str>) -> bool {
        if !task_matches(&self.clone_task, generation) {
            return false;
        }
        let source = self.clone_task.volume_name.clone();
        if !self.finish_volume_operation(generation, &source, VolumeOperation::Cloning) {
            return false;
        }
        self.clone_task = CancellableVolumeTask::default();
        if let Some(created_name) = created_name {
            self.prefer_volume(created_name);
        }
        true
    }

    pub fn fail_clone(&mut self, generation: u64, error: &DockerError) -> bool {
        if !task_matches(&self.clone_task, generation) {
            return false;
        }
        let source = self.clone_task.volume_name.clone();
        if !self.finish_volume_operation(generation, &source, VolumeOperation::Cloning) {
            return false;
        }
        self.clone_task = CancellableVolumeTask::default();
        self.set_operation_error(error);
        true
    }

    pub fn volume_count(&self) -> usize {
        self.source_rows.len()
    }

    pub fn in_use_count(&self) -> usize {
        self.source_rows.iter().filter(|row| row.in_use).count()
    }

    pub fn unused_count(&self) -> usize {
        self.volume_count().saturating_sub(self.in_use_count())
    }

    pub fn size_summary(&self) -> VolumeSizeSummary {
        VolumeSizeSummary::from_rows(&self.source_rows)
    }

    /// Formatted known byte total only. QML combines this with known/unknown
    /// counts; use `size_summary_text` when a complete sentence is needed.
    pub fn known_total_size_text(&self) -> String {
        crate::models::volume_model::format_bytes(self.size_summary().known_total_bytes)
    }

    #[cfg(test)]
    pub fn size_summary_text(&self) -> String {
        self.size_summary().text()
    }

    pub fn creating(&self) -> bool {
        self.global_operation == Some(GlobalVolumeOperation::Creating)
    }

    pub fn pruning(&self) -> bool {
        self.global_operation == Some(GlobalVolumeOperation::Pruning)
    }

    pub fn operation_in_progress(&self) -> bool {
        self.global_operation.is_some() || !self.operations.is_empty()
    }

    fn begin_global_operation(&mut self, operation: GlobalVolumeOperation) -> Option<u64> {
        if self.global_operation.is_some() {
            return None;
        }
        self.global_generation = self.global_generation.wrapping_add(1);
        self.global_operation = Some(operation);
        self.clear_operation_error();
        Some(self.global_generation)
    }

    fn finish_global_operation(
        &mut self,
        generation: u64,
        operation: GlobalVolumeOperation,
    ) -> bool {
        if generation != self.global_generation || self.global_operation != Some(operation) {
            return false;
        }
        self.global_operation = None;
        true
    }

    fn begin_volume_operation(&mut self, name: &str, operation: VolumeOperation) -> Option<u64> {
        if name.is_empty() || !self.source_row_exists(name) || self.operations.contains_key(name) {
            return None;
        }
        self.operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        self.operations.insert(name.to_string(), operation);
        self.operation_generations
            .insert(name.to_string(), generation);
        self.clear_operation_error();
        self.sync_dynamic_roles();
        Some(generation)
    }

    fn finish_volume_operation(
        &mut self,
        generation: u64,
        name: &str,
        operation: VolumeOperation,
    ) -> bool {
        if !self.volume_operation_matches(generation, name, operation) {
            return false;
        }
        self.operations.remove(name);
        self.operation_generations.remove(name);
        self.sync_dynamic_roles();
        true
    }

    fn volume_operation_matches(
        &self,
        generation: u64,
        name: &str,
        operation: VolumeOperation,
    ) -> bool {
        self.operations.get(name).copied() == Some(operation)
            && self.operation_generations.get(name).copied() == Some(generation)
    }

    fn set_operation_error(&mut self, error: &DockerError) {
        let friendly = friendly_volume_error(error, VolumeErrorContext::Operation);
        self.operation_error_kind = friendly.kind.to_string();
        self.operation_error_message = friendly.message.to_string();
    }

    fn clear_operation_error(&mut self) {
        self.operation_error_kind.clear();
        self.operation_error_message.clear();
    }

    fn change_automatic_selection(&mut self, desired: Option<String>) -> Option<u64> {
        match desired {
            Some(desired) if desired == self.selected_volume_name => {
                self.sync_dynamic_roles();
                None
            }
            Some(desired) => {
                self.selected_volume_name = desired;
                self.sync_dynamic_roles();
                Some(self.start_selected_inspect())
            }
            None if self.selected_volume_name.is_empty() => {
                self.sync_dynamic_roles();
                None
            }
            None => {
                self.clear_selection();
                None
            }
        }
    }

    fn start_selected_inspect(&mut self) -> u64 {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        self.detail = None;
        self.detail_state = VolumeDetailState::Loading;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
        self.detail_generation
    }

    fn invalidate_detail(&mut self, state: VolumeDetailState) {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        self.detail = None;
        self.detail_state = state;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
    }

    fn set_selection_without_inspect(&mut self, name: String) {
        self.selected_volume_name = name;
        self.detail = None;
        self.detail_state = VolumeDetailState::None;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
    }

    fn source_row_exists(&self, name: &str) -> bool {
        !name.is_empty() && self.source_rows.iter().any(|row| row.volume_name == name)
    }

    fn visible_row_exists(&self, name: &str) -> bool {
        !name.is_empty() && self.visible_rows.iter().any(|row| row.volume_name == name)
    }

    fn rebuild_visible(&mut self) {
        let query = self.search_query.to_lowercase();
        self.visible_rows = self
            .source_rows
            .iter()
            .filter(|row| query.is_empty() || row_matches(row, &query))
            .cloned()
            .collect();
        let mode = self.sort_mode;
        self.visible_rows
            .sort_by(|left, right| compare_rows(left, right, mode));
        self.sync_dynamic_roles();
    }

    fn sync_dynamic_roles(&mut self) {
        let selected = self.selected_volume_name.as_str();
        for row in self
            .source_rows
            .iter_mut()
            .chain(self.visible_rows.iter_mut())
        {
            row.selected = row.volume_name == selected;
            row.operation = self
                .operations
                .get(&row.volume_name)
                .copied()
                .map(VolumeOperation::as_str)
                .unwrap_or_default()
                .to_string();
            row.busy = !row.operation.is_empty();
        }
    }
}

fn request_cancellation(task: &mut CancellableVolumeTask) -> bool {
    if !task.active || task.cancellation != CancellationState::Cancellable {
        return false;
    }
    task.cancellation = CancellationState::CancellationRequested;
    true
}

fn task_matches(task: &CancellableVolumeTask, generation: u64) -> bool {
    task.active && task.generation == generation
}

fn row_matches(row: &VolumeRow, query: &str) -> bool {
    row.volume_name.to_lowercase().contains(query)
        || row.driver.to_lowercase().contains(query)
        || row.scope.to_lowercase().contains(query)
        || row.mountpoint.to_lowercase().contains(query)
        || row.labels.iter().any(|(key, value)| {
            key.to_lowercase().contains(query) || value.to_lowercase().contains(query)
        })
        || row.options.iter().any(|(key, value)| {
            key.to_lowercase().contains(query) || value.to_lowercase().contains(query)
        })
        || row.containers.iter().any(|container| {
            container.name.to_lowercase().contains(query)
                || container.id.to_lowercase().contains(query)
                || container.short_id.to_lowercase().contains(query)
        })
}

fn compare_rows(left: &VolumeRow, right: &VolumeRow, mode: VolumeSortMode) -> Ordering {
    // Sections remain contiguous in every mode so QML ViewSection can render
    // In Use / Unused headers. Only Unused First reverses section order.
    let section = if mode == VolumeSortMode::UnusedFirst {
        left.in_use.cmp(&right.in_use)
    } else {
        right.in_use.cmp(&left.in_use)
    };
    if section != Ordering::Equal {
        return section;
    }

    let name_ascending = || {
        left.volume_name
            .to_lowercase()
            .cmp(&right.volume_name.to_lowercase())
            .then_with(|| left.volume_name.cmp(&right.volume_name))
    };
    match mode {
        VolumeSortMode::NameAscending
        | VolumeSortMode::InUseFirst
        | VolumeSortMode::UnusedFirst => name_ascending(),
        VolumeSortMode::NameDescending => name_ascending().reverse(),
        VolumeSortMode::NewestFirst => {
            compare_optional(left.created_at, right.created_at, true).then_with(name_ascending)
        }
        VolumeSortMode::OldestFirst => {
            compare_optional(left.created_at, right.created_at, false).then_with(name_ascending)
        }
        VolumeSortMode::LargestFirst => {
            compare_optional(left.size_bytes, right.size_bytes, true).then_with(name_ascending)
        }
        VolumeSortMode::SmallestFirst => {
            compare_optional(left.size_bytes, right.size_bytes, false).then_with(name_ascending)
        }
        VolumeSortMode::MostContainers => right
            .used_by_count
            .cmp(&left.used_by_count)
            .then_with(name_ascending),
        VolumeSortMode::FewestContainers => left
            .used_by_count
            .cmp(&right.used_by_count)
            .then_with(name_ascending),
    }
}

/// Sort optional values with `None` last in both directions.
fn compare_optional<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) if descending => right.cmp(&left),
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[derive(Debug, Clone, Copy)]
enum VolumeErrorContext {
    List,
    Detail,
    Operation,
}

#[derive(Debug, Clone, Copy)]
struct FriendlyVolumeError {
    kind: &'static str,
    message: &'static str,
    list_state: VolumesListState,
}

/// Convert errors to a safe, actionable contract. Daemon payloads, paths, and
/// plugin option values are intentionally not copied into UI strings.
fn friendly_volume_error(error: &DockerError, context: VolumeErrorContext) -> FriendlyVolumeError {
    let result = match error {
        DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => (
            "docker_unavailable",
            "Docker Engine is not available. Check that Docker is running and try again.",
            VolumesListState::DockerUnavailable,
        ),
        DockerError::PermissionDenied => (
            "permission_denied",
            "Permission denied while accessing Docker volumes. Check Docker socket permissions.",
            VolumesListState::PermissionDenied,
        ),
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => (
            "timeout",
            "The Docker volume request timed out. Try again.",
            VolumesListState::Error,
        ),
        DockerError::VolumeNotFound(_) => (
            "volume_not_found",
            "This volume no longer exists. Refresh the volume list.",
            VolumesListState::Error,
        ),
        DockerError::VolumeInUse(_) | DockerError::Conflict(_) => (
            "volume_in_use",
            "Volume is still used by a container and cannot be removed.",
            VolumesListState::Error,
        ),
        DockerError::VolumeAlreadyExists(_) => (
            "volume_already_exists",
            "A volume with this name already exists.",
            VolumesListState::Error,
        ),
        DockerError::VolumeDriverUnavailable(_) => (
            "volume_driver_unavailable",
            "The requested volume driver is unavailable.",
            VolumesListState::Error,
        ),
        DockerError::VolumePluginError(_) => (
            "volume_plugin_error",
            "The volume plugin returned an error.",
            VolumesListState::Error,
        ),
        DockerError::InvalidVolumeName(_) => (
            "invalid_volume_name",
            "Docker rejected the volume name.",
            VolumesListState::Error,
        ),
        DockerError::ExportFailed(_) => (
            "export_failed",
            "Docker could not export the volume.",
            VolumesListState::Error,
        ),
        DockerError::CloneFailed(_) => (
            "clone_failed",
            "Docker could not clone the volume.",
            VolumesListState::Error,
        ),
        DockerError::CleanupFailed(_) => (
            "cleanup_failed",
            "The volume operation failed and temporary resources could not be fully cleaned up.",
            VolumesListState::Error,
        ),
        DockerError::OperationCancelled => (
            "cancelled",
            "The volume operation was cancelled.",
            VolumesListState::Error,
        ),
        DockerError::DestinationPermissionDenied(_) => (
            "destination_permission_denied",
            "The export destination is not writable.",
            VolumesListState::Error,
        ),
        DockerError::DiskFull(_) => (
            "disk_full",
            "There is not enough space at the export destination.",
            VolumesListState::Error,
        ),
        _ if matches!(context, VolumeErrorContext::List) => (
            "docker",
            "Could not load Docker volumes. Try again.",
            VolumesListState::Error,
        ),
        _ if matches!(context, VolumeErrorContext::Detail) => (
            "docker",
            "Volume details are unavailable. Try again.",
            VolumesListState::Error,
        ),
        _ => (
            "volume_operation_failed",
            "Docker could not complete the volume operation. Try again.",
            VolumesListState::Error,
        ),
    };
    FriendlyVolumeError {
        kind: result.0,
        message: result.1,
        list_state: result.2,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use tuxstack_docker_core::{ContainerState, VolumeContainerReference, VolumeUsage};

    use super::*;

    fn container(id: &str, name: &str, state: ContainerState) -> VolumeContainerReference {
        VolumeContainerReference {
            id: id.to_string(),
            short_id: id.chars().take(12).collect(),
            name: name.to_string(),
            state,
            destination: format!("/data/{name}"),
            read_only: false,
            propagation: None,
        }
    }

    fn summary(
        name: &str,
        size: Option<u64>,
        day: Option<u32>,
        containers: &[(&str, &str, ContainerState)],
    ) -> VolumeSummary {
        VolumeSummary {
            name: name.to_string(),
            driver: if name == "plugin" { "nfs" } else { "local" }.to_string(),
            scope: if name == "plugin" { "global" } else { "local" }.to_string(),
            mountpoint: Some(format!("/var/lib/docker/volumes/{name}/_data")),
            created_at: day.map(|day| Utc.with_ymd_and_hms(2026, 3, day, 12, 0, 0).unwrap()),
            labels: BTreeMap::new(),
            options: BTreeMap::new(),
            usage: VolumeUsage {
                size_bytes: size,
                ref_count: containers.len().try_into().ok(),
            },
            used_by: containers
                .iter()
                .map(|(id, name, state)| container(id, name, *state))
                .collect(),
            anonymous: false,
        }
    }

    fn ready_state() -> VolumesState {
        let mut state = VolumesState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary(
                    "zulu",
                    Some(100),
                    Some(1),
                    &[("running-full-id", "web", ContainerState::Running)],
                ),
                summary(
                    "alpha",
                    Some(300),
                    Some(3),
                    &[
                        ("exited-full-id", "db", ContainerState::Exited),
                        ("paused-full-id", "worker", ContainerState::Paused),
                    ],
                ),
                summary("beta", Some(200), Some(2), &[]),
                summary("unknown", None, None, &[]),
            ],
        ));
        state
    }

    fn detail(source: VolumeSummary) -> VolumeDetail {
        VolumeDetail {
            summary: source,
            status: BTreeMap::new(),
        }
    }

    #[test]
    fn initialize_once_and_all_list_states_are_distinct() {
        let mut state = VolumesState::default();
        assert_eq!(state.list_state, VolumesListState::Idle);
        assert!(state.initialize());
        assert!(!state.initialize());

        let generation = state.begin_refresh();
        assert_eq!(state.list_state, VolumesListState::Loading);
        assert!(state.apply_list(generation, &[summary("one", Some(1), Some(1), &[])]));
        assert_eq!(state.list_state, VolumesListState::Ready);

        let generation = state.begin_refresh();
        assert!(state.apply_list(generation, &[]));
        assert_eq!(state.list_state, VolumesListState::Empty);
        assert!(state.selected_volume_name.is_empty());
        assert_eq!(state.detail_state, VolumeDetailState::None);

        let generation = state.begin_refresh();
        assert!(state.apply_list_error(generation, &DockerError::Api("private".into())));
        assert_eq!(state.list_state, VolumesListState::Error);
        assert!(!state.list_error_message.contains("private"));
    }

    #[test]
    fn unavailable_and_permission_have_dedicated_list_states() {
        let mut state = VolumesState::default();
        let generation = state.begin_refresh();
        state.apply_list_error(generation, &DockerError::EngineUnavailable);
        assert_eq!(state.list_state, VolumesListState::DockerUnavailable);
        assert_eq!(state.list_error_kind, "docker_unavailable");

        let generation = state.begin_refresh();
        state.apply_list_error(generation, &DockerError::PermissionDenied);
        assert_eq!(state.list_state, VolumesListState::PermissionDenied);
        assert_eq!(state.list_error_kind, "permission_denied");
    }

    #[test]
    fn first_sorted_row_is_selected_and_stopped_containers_count_as_in_use() {
        let state = ready_state();
        assert_eq!(state.visible_rows[0].volume_name, "alpha");
        assert_eq!(state.selected_volume_name, "alpha");
        assert!(state.visible_rows[0].in_use);
        assert_eq!(state.visible_rows[0].used_by_count, 2);
        assert_eq!(state.in_use_count(), 2);
        assert_eq!(state.unused_count(), 2);
        assert_eq!(state.detail_state, VolumeDetailState::None);
    }

    #[test]
    fn refresh_preserves_selection_and_rejects_stale_list_results() {
        let mut state = ready_state();
        state.select("beta");
        let stale = state.begin_refresh();
        let current = state.begin_refresh();
        assert!(!state.apply_list(stale, &[summary("wrong", Some(1), Some(1), &[])]));
        assert!(state.apply_list(
            current,
            &[
                summary("alpha", Some(1), Some(1), &[]),
                summary("beta", Some(2), Some(2), &[]),
            ]
        ));
        assert_eq!(state.selected_volume_name, "beta");
        assert_eq!(state.detail_state, VolumeDetailState::None);
    }

    #[test]
    fn stage_a_list_without_container_association_keeps_known_in_use_sections() {
        // Simulate the staged pipeline: the first refresh has full summaries
        // (used_by populated), then a later Stage A list arrives with empty
        // container association. The grouping must not flash to all-Unused.
        let mut state = VolumesState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary(
                    "zulu",
                    Some(100),
                    Some(1),
                    &[("running-full-id", "web", ContainerState::Running)],
                ),
                summary("alpha", Some(300), Some(3), &[]),
            ]
        ));
        assert!(state.source_rows.iter().any(|row| row.in_use));
        assert_eq!(state.in_use_count(), 1);
        assert_eq!(state.unused_count(), 1);
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.section == "in_use")
        );

        // Stage A base list carries no container references; the previously
        // known in-use state must be preserved until Stage B patches it.
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("zulu", Some(100), Some(1), &[]),
                summary("alpha", Some(300), Some(3), &[]),
            ]
        ));
        assert!(state.source_rows.iter().any(|row| row.in_use));
        assert_eq!(state.in_use_count(), 1);
        assert_eq!(state.unused_count(), 1);
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.section == "in_use")
        );

        // Stage B enrichment confirms the actual usage and updates the group.
        let mut references = HashMap::new();
        references.insert(
            "zulu".to_string(),
            vec![container("running-full-id", "web", ContainerState::Running)],
        );
        let patched = state.patch_usage(&references, &HashMap::new());
        assert_eq!(patched, 1);
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.in_use)
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.section == "in_use")
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "alpha" && !row.in_use)
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "alpha" && row.section == "unused")
        );
    }

    #[test]
    fn patch_usage_updates_section_when_usage_flips() {
        let mut state = VolumesState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("zulu", Some(100), Some(1), &[]),
                summary("alpha", Some(300), Some(3), &[]),
            ]
        ));
        assert!(state.source_rows.iter().all(|row| !row.in_use));
        assert!(state.source_rows.iter().all(|row| row.section == "unused"));

        // Stage B now reports alpha is used by two containers.
        let mut references = HashMap::new();
        references.insert(
            "alpha".to_string(),
            vec![
                container("a-full-id", "web", ContainerState::Running),
                container("b-full-id", "db", ContainerState::Exited),
            ],
        );
        let patched = state.patch_usage(&references, &HashMap::new());
        assert_eq!(patched, 1);
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "alpha" && row.in_use)
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "alpha" && row.section == "in_use")
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && !row.in_use)
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.section == "unused")
        );

        // Selecting a volume must never change its usage grouping.
        state.select("zulu");
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && !row.in_use)
        );
        assert!(
            state
                .source_rows
                .iter()
                .any(|row| row.volume_name == "zulu" && row.section == "unused")
        );
    }

    #[test]
    fn search_is_local_trimmed_case_insensitive_and_covers_every_required_field() {
        let mut searchable = summary(
            "database",
            Some(10),
            Some(1),
            &[("ABCDEF1234567890", "Postgres", ContainerState::Exited)],
        );
        searchable.driver = "NFS".into();
        searchable.scope = "GLOBAL".into();
        searchable.mountpoint = Some("/mnt/ImportantData".into());
        searchable.labels = BTreeMap::from([("com.Example.Tier".into(), "Storage".into())]);
        searchable.options = BTreeMap::from([("Device".into(), ":/Exports/PG".into())]);
        let mut state = VolumesState::default();
        let generation = state.begin_refresh();
        state.apply_list(
            generation,
            &[searchable, summary("other", Some(1), Some(2), &[])],
        );

        for query in [
            " BASE ",
            "nfs",
            "global",
            "importantdata",
            "tier",
            "STORAGE",
            "device",
            "exports/pg",
            "postgres",
            "abcdef1234567890",
        ] {
            state.set_search_query(query);
            assert_eq!(state.visible_rows.len(), 1, "query {query}");
            assert_eq!(state.visible_rows[0].volume_name, "database");
        }
    }

    #[test]
    fn filtered_selection_uses_first_visible_then_restores_original() {
        let mut state = ready_state();
        state.select("zulu");
        let generation = state.set_search_query(" beta ").unwrap();
        assert_eq!(state.selected_volume_name, "beta");
        assert_eq!(state.selection_before_filter, "zulu");
        assert_eq!(state.detail_generation, generation);
        assert_eq!(state.detail_state, VolumeDetailState::Loading);

        let restored_generation = state.set_search_query("").unwrap();
        assert_eq!(state.selected_volume_name, "zulu");
        assert_eq!(state.detail_generation, restored_generation);
        assert!(state.selection_before_filter.is_empty());

        state.set_search_query("nothing-matches");
        assert!(state.visible_rows.is_empty());
        assert!(state.selected_volume_name.is_empty());
        assert_eq!(state.detail_state, VolumeDetailState::None);
    }

    #[test]
    fn all_ten_sorts_are_stable_grouped_and_unknown_sizes_are_always_last() {
        let mut state = ready_state();
        for value in 0..10 {
            let mode = VolumeSortMode::from_i32(value).unwrap();
            state.set_sort_mode(mode);
            let sections: Vec<_> = state
                .visible_rows
                .iter()
                .map(|row| row.section.as_str())
                .collect();
            let transitions = sections
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .count();
            assert!(transitions <= 1, "mode {mode:?} split a section");
        }
        assert!(VolumeSortMode::from_i32(10).is_none());

        state.set_sort_mode(VolumeSortMode::LargestFirst);
        let unused: Vec<_> = state
            .visible_rows
            .iter()
            .filter(|row| !row.in_use)
            .map(|row| row.volume_name.as_str())
            .collect();
        assert_eq!(unused, vec!["beta", "unknown"]);
        state.set_sort_mode(VolumeSortMode::SmallestFirst);
        let unused: Vec<_> = state
            .visible_rows
            .iter()
            .filter(|row| !row.in_use)
            .map(|row| row.volume_name.as_str())
            .collect();
        assert_eq!(unused, vec!["beta", "unknown"]);
    }

    #[test]
    fn sort_modes_have_expected_in_section_order() {
        let mut state = ready_state();
        let expected_used = [
            (VolumeSortMode::NameAscending, vec!["alpha", "zulu"]),
            (VolumeSortMode::NameDescending, vec!["zulu", "alpha"]),
            (VolumeSortMode::NewestFirst, vec!["alpha", "zulu"]),
            (VolumeSortMode::OldestFirst, vec!["zulu", "alpha"]),
            (VolumeSortMode::LargestFirst, vec!["alpha", "zulu"]),
            (VolumeSortMode::SmallestFirst, vec!["zulu", "alpha"]),
            (VolumeSortMode::MostContainers, vec!["alpha", "zulu"]),
            (VolumeSortMode::FewestContainers, vec!["zulu", "alpha"]),
            (VolumeSortMode::InUseFirst, vec!["alpha", "zulu"]),
        ];
        for (mode, expected) in expected_used {
            state.set_sort_mode(mode);
            assert_eq!(
                state
                    .visible_rows
                    .iter()
                    .filter(|row| row.in_use)
                    .map(|row| row.volume_name.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "mode {mode:?}"
            );
        }
        state.set_sort_mode(VolumeSortMode::UnusedFirst);
        assert!(!state.visible_rows[0].in_use);
    }

    #[test]
    fn detail_state_is_independent_and_stale_details_cannot_win() {
        let mut state = ready_state();
        let alpha = state.begin_selected_inspect().unwrap();
        let beta = state.select("beta").unwrap();
        assert_ne!(alpha, beta);
        assert!(!state.apply_detail(alpha, &detail(summary("alpha", Some(300), Some(3), &[]))));
        assert!(state.apply_detail_error(beta, &DockerError::OperationTimeout));
        assert_eq!(state.list_state, VolumesListState::Ready);
        assert_eq!(state.detail_state, VolumeDetailState::Error);
        assert_eq!(state.detail_error_kind, "timeout");

        let retry = state.begin_selected_inspect().unwrap();
        assert!(state.apply_detail(retry, &detail(summary("beta", Some(200), Some(2), &[]))));
        assert_eq!(state.detail_state, VolumeDetailState::Ready);
        assert_eq!(state.detail.as_ref().unwrap().volume_name, "beta");
    }

    #[test]
    fn deletion_selects_next_then_previous_and_last_leaves_blank_detail() {
        let mut state = VolumesState::default();
        let refresh = state.begin_refresh();
        state.apply_list(
            refresh,
            &[
                summary("alpha", Some(1), Some(1), &[]),
                summary("beta", Some(2), Some(2), &[]),
                summary("charlie", Some(3), Some(3), &[]),
            ],
        );
        state.select("beta");
        let generation = state.remove_local("beta").unwrap();
        assert_eq!(state.selected_volume_name, "charlie");
        assert_eq!(state.detail_generation, generation);
        assert_eq!(state.detail_state, VolumeDetailState::Loading);

        state.remove_local("charlie");
        assert_eq!(state.selected_volume_name, "alpha");
        state.remove_local("alpha");
        assert!(state.selected_volume_name.is_empty());
        assert_eq!(state.detail_state, VolumeDetailState::None);
        assert!(state.detail.is_none());
        assert_eq!(state.list_state, VolumesListState::Empty);
    }

    #[test]
    fn deleting_unselected_volume_preserves_ready_detail() {
        let mut state = ready_state();
        let generation = state.begin_selected_inspect().unwrap();
        assert!(state.apply_detail(
            generation,
            &detail(summary(
                "alpha",
                Some(300),
                Some(3),
                &[("id", "db", ContainerState::Exited)]
            ))
        ));
        assert!(state.remove_local("beta").is_none());
        assert_eq!(state.selected_volume_name, "alpha");
        assert_eq!(state.detail_state, VolumeDetailState::Ready);
        assert!(state.detail.is_some());
    }

    #[test]
    fn create_and_clone_prefer_new_volume_on_refresh_even_under_filter() {
        let mut state = ready_state();
        state.set_search_query("alpha");
        let create = state.begin_create().unwrap();
        assert!(state.begin_prune().is_none());
        assert!(state.finish_create(create, "created"));
        assert!(state.search_query.is_empty());
        let refresh = state.begin_refresh();
        state.apply_list(
            refresh,
            &[
                summary("alpha", Some(1), Some(1), &[]),
                summary("created", Some(2), Some(2), &[]),
            ],
        );
        assert_eq!(state.selected_volume_name, "created");
        assert!(state.preferred_volume_name.is_empty());

        let clone = state.begin_clone("created").unwrap();
        assert!(state.finish_clone(clone, Some("clone")));
        let refresh = state.begin_refresh();
        state.apply_list(
            refresh,
            &[
                summary("created", Some(2), Some(2), &[]),
                summary("clone", Some(2), Some(3), &[]),
            ],
        );
        assert_eq!(state.selected_volume_name, "clone");
    }

    #[test]
    fn per_volume_operations_are_independent_generation_checked_and_sync_roles() {
        let mut state = ready_state();
        let remove = state.begin_remove("alpha").unwrap();
        assert!(state.begin_remove("alpha").is_none());
        let export = state.begin_export("beta").unwrap();
        assert!(state.operation_in_progress());
        let alpha = state
            .visible_rows
            .iter()
            .find(|row| row.volume_name == "alpha")
            .unwrap();
        assert!(alpha.busy);
        assert_eq!(alpha.operation, "removing");
        assert!(!state.finish_remove(remove.wrapping_sub(1), "alpha"));
        assert!(state.finish_remove(remove, "alpha"));
        assert!(state.finish_export(export));
        assert!(!state.operation_in_progress());
        assert!(state.visible_rows.iter().all(|row| !row.busy));
    }

    #[test]
    fn export_and_clone_cancellation_wait_for_task_acknowledgement() {
        let mut state = ready_state();
        let export = state.begin_export("beta").unwrap();
        assert_eq!(
            state.export_task.cancellation,
            CancellationState::Cancellable
        );
        assert!(state.request_cancel_export());
        assert!(!state.request_cancel_export());
        assert_eq!(
            state.export_task.cancellation,
            CancellationState::CancellationRequested
        );
        assert!(state.operations.contains_key("beta"));
        assert!(state.finish_export(export));
        assert_eq!(state.export_task.cancellation, CancellationState::Idle);
        assert!(!state.operations.contains_key("beta"));

        let clone = state.begin_clone("alpha").unwrap();
        assert!(state.request_cancel_clone());
        assert!(!state.finish_clone(clone.wrapping_sub(1), None));
        assert!(state.clone_task.active);
        assert!(state.finish_clone(clone, None));
        assert!(!state.clone_task.active);
        assert!(!state.operation_in_progress());
    }

    #[test]
    fn operation_failures_clear_busy_and_do_not_leak_daemon_payload() {
        let mut state = ready_state();
        let generation = state.begin_remove("alpha").unwrap();
        assert!(!state.fail_volume_operation(
            generation.wrapping_sub(1),
            "alpha",
            VolumeOperation::Removing,
            &DockerError::Conflict("secret daemon payload".into()),
        ));
        assert!(state.fail_volume_operation(
            generation,
            "alpha",
            VolumeOperation::Removing,
            &DockerError::Conflict("secret daemon payload".into()),
        ));
        assert_eq!(state.operation_error_kind, "volume_in_use");
        assert!(!state.operation_error_message.contains("secret"));
        assert!(!state.operations.contains_key("alpha"));

        let generation = state.begin_prune().unwrap();
        assert_eq!(state.prune_cancellation, CancellationState::Cancellable);
        assert!(state.request_cancel_prune());
        assert!(state.fail_global_operation(generation, &DockerError::PermissionDenied));
        assert!(!state.pruning());
        assert_eq!(state.prune_cancellation, CancellationState::Idle);
        assert_eq!(state.operation_error_kind, "permission_denied");
    }

    #[test]
    fn cancellable_task_failures_acknowledge_and_clear_busy() {
        let mut state = ready_state();
        let export = state.begin_export("beta").unwrap();
        assert!(state.request_cancel_export());
        assert!(state.fail_export(export, &DockerError::OperationCancelled));
        assert!(!state.export_task.active);
        assert!(!state.operations.contains_key("beta"));
        assert_eq!(state.operation_error_kind, "cancelled");

        let clone = state.begin_clone("alpha").unwrap();
        assert!(state.fail_clone(clone, &DockerError::OperationTimeout));
        assert!(!state.clone_task.active);
        assert!(!state.operations.contains_key("alpha"));
        assert!(!state.operation_in_progress());
    }

    #[test]
    fn aggregate_size_counts_known_unknown_and_does_not_fake_unknown_zero() {
        let state = ready_state();
        let size = state.size_summary();
        assert_eq!(size.known_total_bytes, 600);
        assert_eq!(size.known_count, 3);
        assert_eq!(size.unknown_count, 1);
        assert_eq!(state.known_total_size_text(), "600 B");
        assert_eq!(state.size_summary_text(), "600 B known · 1 volume unknown");
    }
}
