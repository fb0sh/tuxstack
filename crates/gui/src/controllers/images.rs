//! Pure state/controller logic for Docker images.

use std::cmp::Ordering;
use std::collections::HashMap;

use tuxstack_docker_core::{ImageDetail, ImagePullProgress, ImageSummary};

use crate::app_state::map_docker_error;
use crate::models::image_model::{ImageDetailView, ImageRow, format_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionChange {
    Selected(u64),
    Deselected,
    Ignored,
}

/// List-level state. Detail inspection never changes this state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ImagesListState {
    #[default]
    Loading,
    Ready,
    Empty,
    Error,
}

// Keep the earlier internal names available while callers migrate to the
// canonical UIFIX contract above.
#[allow(non_upper_case_globals)]
impl ImagesListState {
    const LoadingList: Self = Self::Loading;
    const DockerError: Self = Self::Error;
}

/// Detail-level state. List refresh failures never become detail failures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageDetailState {
    #[default]
    None,
    Loading,
    Ready,
    Error,
}

// Compatibility aliases keep the bridge/test surface stable while the public
// controller contract uses the explicit list/detail state names above.
type ImagesPageStatus = ImagesListState;
type ImageDetailStatus = ImageDetailState;

/// The eight QML-visible sort modes. Numeric values are stable API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ImageSortMode {
    NameAscending = 0,
    NameDescending = 1,
    NewestFirst = 2,
    OldestFirst = 3,
    LargestFirst = 4,
    SmallestFirst = 5,
    #[default]
    UsedFirst = 6,
    UnusedFirst = 7,
}

impl ImageSortMode {
    #[cfg(test)]
    pub fn from_i32(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::NameAscending,
            1 => Self::NameDescending,
            2 => Self::NewestFirst,
            3 => Self::OldestFirst,
            4 => Self::LargestFirst,
            5 => Self::SmallestFirst,
            6 => Self::UsedFirst,
            7 => Self::UnusedFirst,
            _ => return None,
        })
    }
}

/// State for pull progress, kept independently from the model rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PullState {
    pub active: bool,
    pub reference: String,
    pub layer_id: String,
    pub status: String,
    pub current: u64,
    pub total: u64,
    pub percent: f64,
}

/// State for streaming export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportState {
    pub active: bool,
    pub image_id: String,
    pub destination: String,
    pub bytes_written: u64,
}

/// Pure, Qt-free state machine backing `ImageListModel`.
#[derive(Debug, Clone)]
pub struct ImagesState {
    pub source_rows: Vec<ImageRow>,
    pub visible_rows: Vec<ImageRow>,
    pub busy: HashMap<String, String>,
    pub busy_generations: HashMap<String, u64>,
    pub search_query: String,
    pub sort_mode: ImageSortMode,
    pub selected_image_id: String,
    pub detail: Option<ImageDetailView>,
    pub refresh_generation: u64,
    pub detail_generation: u64,
    pub operation_generation: u64,
    pub pull_generation: u64,
    pub export_generation: u64,
    pub preferred_reference: String,
    pub initialized: bool,
    pub status: ImagesListState,
    pub status_text: String,
    pub error_kind: String,
    pub detail_status: ImageDetailState,
    pub detail_error: String,
    pub detail_error_kind: String,
    pub operation_in_progress: bool,
    pub pull: PullState,
    pub export: ExportState,
}

impl Default for ImagesState {
    fn default() -> Self {
        Self {
            source_rows: Vec::new(),
            visible_rows: Vec::new(),
            busy: HashMap::new(),
            busy_generations: HashMap::new(),
            search_query: String::new(),
            sort_mode: ImageSortMode::default(),
            selected_image_id: String::new(),
            detail: None,
            refresh_generation: 0,
            detail_generation: 0,
            operation_generation: 0,
            pull_generation: 0,
            export_generation: 0,
            preferred_reference: String::new(),
            initialized: false,
            status: ImagesPageStatus::LoadingList,
            status_text: String::new(),
            error_kind: String::new(),
            detail_status: ImageDetailStatus::None,
            detail_error: String::new(),
            detail_error_kind: String::new(),
            operation_in_progress: false,
            pull: PullState::default(),
            export: ExportState::default(),
        }
    }
}

impl ImagesState {
    /// Mark the controller initialized. Returns true exactly once.
    pub fn initialize(&mut self) -> bool {
        if self.initialized {
            return false;
        }
        self.initialized = true;
        true
    }

    pub fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.detail_generation = self.detail_generation.wrapping_add(1);
        self.status = ImagesPageStatus::LoadingList;
        self.status_text.clear();
        self.error_kind.clear();
        self.detail = None;
        self.detail_status = ImageDetailStatus::None;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        self.refresh_generation
    }

    pub fn apply_list(&mut self, generation: u64, summaries: &[ImageSummary]) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        let previous = self.selected_image_id.clone();
        self.source_rows = summaries.iter().map(ImageRow::from).collect();
        self.rebuild_visible();
        self.status = if self.source_rows.is_empty() {
            ImagesPageStatus::Empty
        } else {
            ImagesPageStatus::Ready
        };
        self.status_text.clear();
        self.error_kind.clear();
        if !self.preferred_reference.is_empty() {
            let preferred = self
                .source_rows
                .iter()
                .find(|row| {
                    row.display_name == self.preferred_reference
                        || row
                            .repo_tags
                            .iter()
                            .any(|tag| tag == &self.preferred_reference)
                })
                .map(|row| row.image_id.clone());
            self.preferred_reference.clear();
            if let Some(preferred) = preferred {
                self.set_refresh_selection(preferred);
            } else {
                self.restore_after_refresh(&previous);
            }
        } else {
            self.restore_after_refresh(&previous);
        }
        true
    }

    pub fn apply_list_error(
        &mut self,
        generation: u64,
        error: &tuxstack_docker_core::DockerError,
    ) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        let error = map_docker_error(error);
        self.source_rows.clear();
        self.visible_rows.clear();
        self.clear_selection();
        self.status = ImagesPageStatus::DockerError;
        self.error_kind = error.kind().to_string();
        self.status_text = error.user_message();
        true
    }

    pub fn set_search_query(&mut self, query: &str) {
        self.search_query = query.trim().to_string();
        let selected = self.selected_image_id.clone();
        self.rebuild_visible();
        self.restore_selection(&selected);
    }

    pub fn set_sort_mode(&mut self, mode: ImageSortMode) {
        self.sort_mode = mode;
        self.rebuild_visible();
    }

    pub fn select(&mut self, id: &str) -> Option<u64> {
        if !self.source_rows.iter().any(|row| row.image_id == id) {
            return None;
        }
        self.selected_image_id = id.to_string();
        self.detail = None;
        self.detail_status = ImageDetailStatus::Loading;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        self.detail_generation = self.detail_generation.wrapping_add(1);
        Some(self.detail_generation)
    }

    pub fn toggle_selection(&mut self, id: &str) -> SelectionChange {
        if self.selected_image_id == id {
            self.clear_selection();
            return SelectionChange::Deselected;
        }
        self.select(id)
            .map(SelectionChange::Selected)
            .unwrap_or(SelectionChange::Ignored)
    }

    pub fn clear_selection(&mut self) {
        self.selected_image_id.clear();
        self.detail = None;
        self.detail_status = ImageDetailStatus::None;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        self.detail_generation = self.detail_generation.wrapping_add(1);
    }

    pub fn apply_detail(&mut self, generation: u64, detail: &ImageDetail) -> bool {
        if generation != self.detail_generation || detail.summary.id != self.selected_image_id {
            return false;
        }
        let architecture = match (&detail.architecture, &detail.variant) {
            (Some(architecture), Some(variant))
                if !architecture.is_empty() && !variant.is_empty() =>
            {
                format!("{architecture}/{variant}")
            }
            (Some(architecture), _) if !architecture.is_empty() => architecture.clone(),
            _ => "unknown".to_string(),
        };
        if let Some(row) = self
            .source_rows
            .iter_mut()
            .find(|row| row.image_id == detail.summary.id)
        {
            row.architecture = architecture;
        }
        self.rebuild_visible();
        self.detail = Some(ImageDetailView::from(detail));
        self.detail_status = ImageDetailStatus::Ready;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        true
    }

    pub fn apply_detail_error(&mut self, generation: u64, kind: String, message: String) -> bool {
        if generation != self.detail_generation {
            return false;
        }
        self.detail = None;
        self.detail_status = ImageDetailStatus::Error;
        self.detail_error_kind = kind;
        self.detail_error = message;
        true
    }

    /// Mark an image operation as busy. Duplicate operations are rejected.
    pub fn begin_operation(&mut self, id: &str, operation: &str) -> Option<u64> {
        if self.busy.contains_key(id) {
            return None;
        }
        self.operation_generation = self.operation_generation.wrapping_add(1);
        self.busy.insert(id.to_string(), operation.to_string());
        self.busy_generations
            .insert(id.to_string(), self.operation_generation);
        self.operation_in_progress = true;
        Some(self.operation_generation)
    }

    pub fn finish_operation(&mut self, id: &str, generation: u64) -> bool {
        if self.busy_generations.get(id).copied() != Some(generation) {
            return false;
        }
        self.busy.remove(id);
        self.busy_generations.remove(id);
        self.operation_in_progress =
            !self.busy.is_empty() || self.pull.active || self.export.active;
        true
    }

    /// Optimistically remove a deleted row and choose its nearest neighbour.
    pub fn remove_local(&mut self, id: &str) {
        let old_index = self
            .visible_rows
            .iter()
            .position(|row| row.image_id == id)
            .unwrap_or(0);
        self.source_rows.retain(|row| row.image_id != id);
        self.rebuild_visible();
        if self.selected_image_id == id {
            self.detail_generation = self.detail_generation.wrapping_add(1);
            self.detail = None;
            self.detail_error.clear();
            self.detail_error_kind.clear();
            self.selected_image_id = self
                .visible_rows
                .get(old_index.min(self.visible_rows.len().saturating_sub(1)))
                .map(|row| row.image_id.clone())
                .unwrap_or_default();
            self.detail_status = if self.selected_image_id.is_empty() {
                ImageDetailStatus::None
            } else {
                ImageDetailStatus::Loading
            };
        }
        self.status = if self.source_rows.is_empty() {
            ImagesPageStatus::Empty
        } else {
            ImagesPageStatus::Ready
        };
    }

    pub fn begin_pull(&mut self, reference: &str) -> Option<u64> {
        let reference = reference.trim();
        if reference.is_empty() || self.pull.active {
            return None;
        }
        self.pull_generation = self.pull_generation.wrapping_add(1);
        self.pull = PullState {
            active: true,
            reference: reference.to_string(),
            status: "Resolving".to_string(),
            ..Default::default()
        };
        self.operation_in_progress = true;
        Some(self.pull_generation)
    }

    pub fn apply_pull_progress(&mut self, generation: u64, progress: &ImagePullProgress) -> bool {
        if generation != self.pull_generation
            || !self.pull.active
            || progress.image_reference != self.pull.reference
        {
            return false;
        }
        self.pull.layer_id = progress.layer_id.clone().unwrap_or_default();
        self.pull.status = progress.status.clone();
        self.pull.current = progress.current.unwrap_or_default();
        self.pull.total = progress.total.unwrap_or_default();
        self.pull.percent = progress.percent.unwrap_or(-1.0);
        // "Pull complete" may describe one layer, not the entire image. The
        // controller only completes when docker-core's stream terminates.
        true
    }

    pub fn finish_pull(&mut self, generation: u64, prefer_pulled_image: bool) -> bool {
        if generation != self.pull_generation {
            return false;
        }
        if prefer_pulled_image {
            self.preferred_reference = self.pull.reference.clone();
        }
        self.pull.active = false;
        self.operation_in_progress = !self.busy.is_empty() || self.export.active;
        true
    }

    pub fn begin_export(&mut self, id: &str, destination: &str) -> Option<u64> {
        if self.export.active || id.is_empty() || destination.is_empty() {
            return None;
        }
        self.export_generation = self.export_generation.wrapping_add(1);
        self.export = ExportState {
            active: true,
            image_id: id.to_string(),
            destination: destination.to_string(),
            bytes_written: 0,
        };
        self.operation_in_progress = true;
        Some(self.export_generation)
    }

    pub fn update_export_bytes(&mut self, generation: u64, bytes: u64) -> bool {
        if generation != self.export_generation || !self.export.active {
            return false;
        }
        self.export.bytes_written = bytes;
        true
    }

    pub fn finish_export(&mut self, generation: u64) -> bool {
        if generation != self.export_generation {
            return false;
        }
        self.export.active = false;
        self.operation_in_progress = !self.busy.is_empty() || self.pull.active;
        true
    }

    pub fn total_image_count(&self) -> usize {
        self.source_rows.len()
    }

    pub fn in_use_count(&self) -> usize {
        self.source_rows.iter().filter(|row| row.in_use).count()
    }

    pub fn unused_count(&self) -> usize {
        self.total_image_count() - self.in_use_count()
    }

    pub fn total_size_bytes(&self) -> u64 {
        let mut unique = HashMap::<String, u64>::new();
        for row in &self.source_rows {
            let id = tuxstack_docker_core::mapping::images::normalize_image_id(&row.image_id);
            unique
                .entry(id)
                .and_modify(|size| *size = (*size).max(row.size_bytes))
                .or_insert(row.size_bytes);
        }
        unique
            .values()
            .fold(0_u64, |total, size| total.saturating_add(*size))
    }

    pub fn total_size_text(&self) -> String {
        format_bytes(self.total_size_bytes())
    }

    fn restore_selection(&mut self, selected: &str) {
        if !selected.is_empty() && self.source_rows.iter().any(|row| row.image_id == selected) {
            self.selected_image_id = selected.to_string();
        } else {
            self.clear_selection();
        }
    }

    /// Restore a valid prior selection after a Docker refresh, otherwise pick
    /// the first In Use row and then the first Unused row.
    fn restore_after_refresh(&mut self, selected: &str) {
        if !selected.is_empty() && self.source_rows.iter().any(|row| row.image_id == selected) {
            self.selected_image_id = selected.to_string();
            return;
        }
        let fallback = self
            .visible_rows
            .iter()
            .find(|row| row.in_use)
            .or_else(|| self.visible_rows.first())
            .map(|row| row.image_id.clone());
        if let Some(fallback) = fallback {
            self.set_refresh_selection(fallback);
        } else {
            self.clear_selection();
        }
    }

    fn set_refresh_selection(&mut self, image_id: String) {
        self.selected_image_id = image_id;
        self.detail = None;
        self.detail_status = ImageDetailStatus::None;
        self.detail_error.clear();
        self.detail_error_kind.clear();
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
    }
}

fn row_matches(row: &ImageRow, query: &str) -> bool {
    row.display_name.to_lowercase().contains(query)
        || row.image_id.to_lowercase().contains(query)
        || row.short_id.to_lowercase().contains(query)
        || row
            .repo_tags
            .iter()
            .chain(row.repo_digests.iter())
            .any(|value| value.to_lowercase().contains(query))
        || row.labels.iter().any(|(key, value)| {
            key.to_lowercase().contains(query) || value.to_lowercase().contains(query)
        })
        || row.architecture.to_lowercase().contains(query)
        || row.containers.iter().any(|container| {
            container.name.to_lowercase().contains(query)
                || container.short_id.to_lowercase().contains(query)
        })
}

fn compare_rows(left: &ImageRow, right: &ImageRow, mode: ImageSortMode) -> Ordering {
    // Keep each section contiguous for QML's ViewSection delegate. The two
    // section-order modes choose which group comes first; all other modes use
    // the documented default of In Use first and sort inside each group.
    let section = if mode == ImageSortMode::UnusedFirst {
        left.in_use.cmp(&right.in_use)
    } else {
        right.in_use.cmp(&left.in_use)
    };
    if section != Ordering::Equal {
        return section;
    }
    let name = || {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
    };
    let newest = || right.created_at.cmp(&left.created_at).then_with(name);
    match mode {
        ImageSortMode::NameAscending => name(),
        ImageSortMode::NameDescending => name().reverse(),
        ImageSortMode::NewestFirst => newest(),
        ImageSortMode::OldestFirst => left.created_at.cmp(&right.created_at).then_with(name),
        ImageSortMode::LargestFirst => right.size_bytes.cmp(&left.size_bytes).then_with(newest),
        ImageSortMode::SmallestFirst => left.size_bytes.cmp(&right.size_bytes).then_with(newest),
        ImageSortMode::UsedFirst | ImageSortMode::UnusedFirst => newest(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use tuxstack_docker_core::{DockerError, ImageDetail, ImageSummary};

    use super::*;

    fn summary(id: &str, name: &str, size: u64, in_use: bool, day: u32) -> ImageSummary {
        ImageSummary {
            id: format!("sha256:{id}"),
            short_id: id.to_string(),
            repo_tags: vec![name.to_string()],
            repo_digests: vec![],
            display_name: name.to_string(),
            created_at: Some(Utc.with_ymd_and_hms(2024, 1, day, 0, 0, 0).unwrap()),
            size_bytes: size,
            shared_size_bytes: None,
            virtual_size_bytes: Some(size),
            labels: BTreeMap::new(),
            containers: vec![],
            in_use,
        }
    }

    fn detail(id: &str, name: &str, size: u64, in_use: bool, day: u32) -> ImageDetail {
        ImageDetail {
            summary: summary(id, name, size, in_use, day),
            architecture: Some("amd64".into()),
            os: Some("linux".into()),
            variant: None,
            author: None,
            docker_version: None,
            comment: None,
            command: vec![],
            entrypoint: vec![],
            environment: vec![],
            working_dir: None,
            user: None,
            stop_signal: None,
            shell: vec![],
            labels: BTreeMap::new(),
            root_fs_layers: vec![],
        }
    }

    fn ready_state() -> ImagesState {
        let mut state = ImagesState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("aaa", "zulu:latest", 100, false, 1),
                summary("bbb", "alpha:latest", 300, true, 3),
                summary("ccc", "beta:latest", 200, true, 2),
            ],
        ));
        state
    }

    #[test]
    fn loading_transitions_to_ready_empty_and_error() {
        let mut state = ImagesState::default();
        let generation = state.begin_refresh();
        assert_eq!(state.status, ImagesPageStatus::LoadingList);
        assert!(state.apply_list(generation, &[summary("a", "a:1", 1, false, 1)]));
        assert_eq!(state.status, ImagesPageStatus::Ready);

        let generation = state.begin_refresh();
        assert!(state.apply_list(generation, &[]));
        assert_eq!(state.status, ImagesPageStatus::Empty);

        let generation = state.begin_refresh();
        assert!(state.apply_list_error(generation, &DockerError::Api("boom".into())));
        assert_eq!(state.status, ImagesPageStatus::DockerError);
        assert_eq!(state.error_kind, "docker");
    }

    #[test]
    fn maps_unavailable_and_permission_states() {
        let mut state = ImagesState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list_error(generation, &DockerError::EngineUnavailable));
        assert_eq!(state.status, ImagesPageStatus::DockerError);
        assert_eq!(state.error_kind, "docker_unavailable");

        let generation = state.begin_refresh();
        assert!(state.apply_list_error(generation, &DockerError::PermissionDenied));
        assert_eq!(state.status, ImagesPageStatus::DockerError);
        assert_eq!(state.error_kind, "permission_denied");
    }

    #[test]
    fn search_is_local_trimmed_and_comprehensive() {
        let mut image = summary("abc", "registry/example:1", 1, false, 1);
        image
            .labels
            .insert("org.example.channel".into(), "Nightly".into());
        let mut state = ImagesState::default();
        let generation = state.begin_refresh();
        state.apply_list(generation, &[image, summary("def", "other:1", 1, false, 1)]);
        state.set_search_query("  NIGHTLY ");
        assert_eq!(state.visible_rows.len(), 1);
        assert_eq!(state.visible_rows[0].short_id, "abc");
        state.set_search_query("");
        assert_eq!(state.visible_rows.len(), 2);
    }

    #[test]
    fn all_eight_sort_modes_are_stable() {
        let mut state = ready_state();
        let expected_first = [
            "alpha:latest",
            "beta:latest",
            "alpha:latest",
            "beta:latest",
            "alpha:latest",
            "beta:latest",
            "alpha:latest",
            "zulu:latest",
        ];
        for (value, expected) in expected_first.into_iter().enumerate() {
            state.set_sort_mode(ImageSortMode::from_i32(value as i32).unwrap());
            assert_eq!(state.visible_rows[0].display_name, expected);
        }
        assert!(ImageSortMode::from_i32(8).is_none());
    }

    #[test]
    fn initial_selection_prefers_in_use_and_clicking_it_toggles_it() {
        let mut state = ready_state();
        assert_eq!(state.selected_image_id, "sha256:bbb");
        assert!(state.detail.is_none());
        assert_eq!(state.detail_status, ImageDetailStatus::None);

        let generation = match state.toggle_selection("sha256:ccc") {
            SelectionChange::Selected(generation) => generation,
            change => panic!("expected selection, got {change:?}"),
        };
        assert_eq!(state.selected_image_id, "sha256:ccc");
        assert_eq!(state.detail_status, ImageDetailStatus::Loading);

        assert_eq!(
            state.toggle_selection("sha256:ccc"),
            SelectionChange::Deselected
        );
        assert!(state.selected_image_id.is_empty());
        assert!(state.detail.is_none());
        assert_eq!(state.detail_status, ImageDetailStatus::None);
        assert!(state.detail_generation > generation);
        assert!(!state.apply_detail(generation, &detail("ccc", "beta:latest", 200, true, 2)));
        assert!(state.detail.is_none());
    }

    #[test]
    fn first_unused_is_selected_when_no_image_is_in_use() {
        let mut state = ImagesState::default();
        let generation = state.begin_refresh();
        state.apply_list(
            generation,
            &[
                summary("aaa", "alpha:latest", 100, false, 1),
                summary("bbb", "beta:latest", 200, false, 2),
            ],
        );
        assert_eq!(state.selected_image_id, "sha256:bbb");

        let generation = state.begin_refresh();
        state.apply_list(generation, &[]);
        assert!(state.selected_image_id.is_empty());
        assert_eq!(state.detail_status, ImageDetailStatus::None);
    }

    #[test]
    fn refresh_fallback_prefers_in_use_even_when_unused_sort_is_active() {
        let mut state = ImagesState::default();
        state.set_sort_mode(ImageSortMode::UnusedFirst);
        let generation = state.begin_refresh();
        state.apply_list(
            generation,
            &[
                summary("unused", "unused:latest", 100, false, 3),
                summary("used", "used:latest", 200, true, 1),
            ],
        );
        assert_eq!(state.visible_rows[0].image_id, "sha256:unused");
        assert_eq!(state.selected_image_id, "sha256:used");
    }

    #[test]
    fn initialize_is_idempotent() {
        let mut state = ImagesState::default();
        assert!(state.initialize());
        assert!(!state.initialize());
    }

    #[test]
    fn refresh_preserves_selection_and_ignores_stale_results() {
        let mut state = ready_state();
        state.select("sha256:ccc");
        let old = state.begin_refresh();
        let current = state.begin_refresh();
        assert!(!state.apply_list(old, &[summary("old", "old", 1, false, 1)]));
        assert!(state.apply_list(current, &[summary("ccc", "beta:latest", 200, true, 2)]));
        assert_eq!(state.selected_image_id, "sha256:ccc");
    }

    #[test]
    fn deletion_selects_adjacent_row_and_clears_busy() {
        let mut state = ready_state();
        let selected = state.visible_rows[1].image_id.clone();
        let expected_next = state.visible_rows[2].image_id.clone();
        state.selected_image_id = selected.clone();
        let generation = state.begin_operation(&selected, "removing").unwrap();
        state.remove_local(&selected);
        assert!(state.finish_operation(&selected, generation));
        assert_eq!(state.selected_image_id, expected_next);
        assert_eq!(state.detail_status, ImageDetailStatus::Loading);
        assert_eq!(state.visible_rows.len(), 2);
        assert!(!state.operation_in_progress);

        let last = state.visible_rows.last().unwrap().image_id.clone();
        state.selected_image_id = last.clone();
        state.remove_local(&last);
        assert_eq!(state.selected_image_id, state.visible_rows[0].image_id);

        let only = state.visible_rows[0].image_id.clone();
        state.selected_image_id = only.clone();
        state.remove_local(&only);
        assert!(state.selected_image_id.is_empty());
        assert_eq!(state.detail_status, ImageDetailStatus::None);
    }

    #[test]
    fn stale_detail_cannot_replace_new_selection() {
        let mut state = ready_state();
        let generation_a = state.select("sha256:bbb").unwrap();
        let generation_b = state.select("sha256:ccc").unwrap();
        assert_ne!(generation_a, generation_b);
        let wrong = detail("bbb", "alpha:latest", 300, true, 3);
        assert!(!state.apply_detail(generation_a, &wrong));
        assert!(state.detail.is_none());
    }

    #[test]
    fn pull_progress_cancel_and_export_cancel_clear_busy() {
        let mut state = ImagesState::default();
        let pull_generation = state.begin_pull("alpine:latest").unwrap();
        state.apply_pull_progress(
            pull_generation,
            &ImagePullProgress {
                image_reference: "alpine:latest".into(),
                layer_id: Some("layer".into()),
                status: "Downloading".into(),
                current: Some(5),
                total: Some(10),
                percent: Some(50.0),
                completed: false,
            },
        );
        assert_eq!(state.pull.percent, 50.0);
        assert!(state.finish_pull(pull_generation, false));
        assert!(!state.operation_in_progress);
        let newer_pull = state.begin_pull("busybox:latest").unwrap();
        assert!(!state.finish_pull(pull_generation, false));
        assert!(state.pull.active);
        assert!(state.finish_pull(newer_pull, false));

        let export_generation = state.begin_export("sha256:a", "/tmp/a.tar").unwrap();
        assert!(state.update_export_bytes(export_generation, 42));
        assert_eq!(state.export.bytes_written, 42);
        assert!(state.finish_export(export_generation));
        assert!(!state.operation_in_progress);
        let newer_export = state.begin_export("sha256:b", "/tmp/b.tar").unwrap();
        assert!(!state.finish_export(export_generation));
        assert!(state.export.active);
        assert!(state.finish_export(newer_export));
    }

    #[test]
    fn unique_ids_use_the_maximum_size_once_for_total_size() {
        let mut state = ready_state();
        let mut duplicate = state.source_rows[0].clone();
        duplicate.image_id = duplicate.image_id.replacen("sha256:", "sha256-", 1);
        duplicate.size_bytes = 400;
        state.source_rows.push(duplicate);
        assert_eq!(state.total_image_count(), 4);
        assert_eq!(state.total_size_bytes(), 900);
    }

    #[test]
    fn detail_error_does_not_change_ready_list_state() {
        let mut state = ready_state();
        let generation = state.select("sha256:bbb").unwrap();
        assert!(state.apply_detail_error(
            generation,
            "timeout".into(),
            "Loading image details timed out. Try again.".into(),
        ));
        assert_eq!(state.status, ImagesPageStatus::Ready);
        assert_eq!(state.source_rows.len(), 3);
        assert_eq!(state.detail_status, ImageDetailStatus::Error);
        assert_eq!(state.detail_error_kind, "timeout");
    }
}
