//! Pure state machine for point-in-time container filesystem browsing.
//!
//! Docker I/O and cancellation tokens live in the bridge. This controller
//! owns snapshot TTL semantics, request generations, navigation, filtering,
//! sorting, pagination state, and mount-overlay presentation rules.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tuxstack_docker_core::{
    ContainerDirectoryPage, ContainerDirectoryQuery, ContainerDirectorySort,
    ContainerDirectorySortOrder, ContainerFilesystemEntry, ContainerFilesystemEntryType,
    ContainerFilesystemOrigin, ContainerMountOverlay, ContainerMountOverlayKind,
};

pub const CONTAINER_SNAPSHOT_TTL: Duration = Duration::from_secs(10);
pub const CONTAINER_DIRECTORY_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainerFilesState {
    #[default]
    Idle,
    LoadingSnapshot,
    LoadingDirectory,
    Ready,
    Empty,
    Error,
}

impl ContainerFilesState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::LoadingSnapshot => "loading_snapshot",
            Self::LoadingDirectory => "loading_directory",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotDecision {
    Reuse,
    RefreshEmpty,
    RefreshKeepingSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFileSortColumn {
    Name,
    Modified,
    Size,
    Type,
}

impl ContainerFileSortColumn {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Type => "type",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "modified" | "date" | "mtime" => Self::Modified,
            "size" => Self::Size,
            "type" | "kind" => Self::Type,
            _ => Self::Name,
        }
    }

    fn core(self) -> ContainerDirectorySort {
        match self {
            Self::Name => ContainerDirectorySort::Name,
            Self::Modified => ContainerDirectorySort::Modified,
            Self::Size => ContainerDirectorySort::Size,
            Self::Type => ContainerDirectorySort::Type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountAction {
    Volume { name: String },
    Bind { source: String },
    Tmpfs,
    Other,
}

#[derive(Debug, Clone)]
pub struct ContainerFilesControllerState {
    pub state: ContainerFilesState,
    pub active: bool,
    pub container_id: String,
    pub current_path: String,
    pub history: Vec<String>,
    pub show_hidden: bool,
    pub search_query: String,
    pub sort_column: ContainerFileSortColumn,
    pub sort_descending: bool,
    pub entries: Vec<ContainerFilesystemEntry>,
    pub mount_overlays: Vec<ContainerMountOverlay>,
    pub next_cursor: Option<String>,
    pub total_visible: usize,
    pub snapshot_generated_at: Option<DateTime<Utc>>,
    pub snapshot_expires_at: Option<DateTime<Utc>>,
    pub snapshot_invalidated: bool,
    pub snapshot_in_flight: bool,
    pub error_kind: String,
    pub error_message: String,
    pub selected_path: Option<String>,
    pub snapshot_generation: u64,
    pub list_generation: u64,
    pub preview_generation: u64,
    pub save_generation: u64,
}

impl Default for ContainerFilesControllerState {
    fn default() -> Self {
        Self {
            state: ContainerFilesState::Idle,
            active: false,
            container_id: String::new(),
            current_path: "/".into(),
            history: Vec::new(),
            show_hidden: false,
            search_query: String::new(),
            sort_column: ContainerFileSortColumn::Name,
            sort_descending: false,
            entries: Vec::new(),
            mount_overlays: Vec::new(),
            next_cursor: None,
            total_visible: 0,
            snapshot_generated_at: None,
            snapshot_expires_at: None,
            snapshot_invalidated: false,
            snapshot_in_flight: false,
            error_kind: String::new(),
            error_message: String::new(),
            selected_path: None,
            snapshot_generation: 0,
            list_generation: 0,
            preview_generation: 0,
            save_generation: 0,
        }
    }
}

impl ContainerFilesControllerState {
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Switch the authoritative selection. A different container invalidates
    /// and removes the previous container's snapshot; leaving the tab does not.
    pub fn select_container(&mut self, container_id: &str) -> bool {
        let container_id = container_id.trim();
        if self.container_id == container_id {
            return false;
        }
        self.cancel_all();
        self.container_id = container_id.to_string();
        self.reset_snapshot();
        true
    }

    pub fn clear_selection(&mut self) {
        self.cancel_all();
        self.container_id.clear();
        self.reset_snapshot();
    }

    pub fn snapshot_decision(&self, now: DateTime<Utc>, force: bool) -> SnapshotDecision {
        if self.snapshot_generated_at.is_none() {
            return SnapshotDecision::RefreshEmpty;
        }
        if force || !self.snapshot_is_fresh_at(now) {
            SnapshotDecision::RefreshKeepingSnapshot
        } else {
            SnapshotDecision::Reuse
        }
    }

    pub fn snapshot_is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        !self.snapshot_invalidated
            && self
                .snapshot_expires_at
                .is_some_and(|expires| now <= expires)
    }

    /// Start one snapshot request. The bridge must cancel its previous token
    /// before using the returned generation, providing single-active-request
    /// semantics even for repeated manual refreshes.
    pub fn begin_snapshot(&mut self, now: DateTime<Utc>, force: bool) -> Option<u64> {
        if self.container_id.is_empty() || self.snapshot_in_flight {
            return None;
        }
        if !force && self.snapshot_decision(now, false) == SnapshotDecision::Reuse {
            return None;
        }
        self.snapshot_generation = self.snapshot_generation.wrapping_add(1);
        self.list_generation = self.list_generation.wrapping_add(1);
        self.snapshot_in_flight = true;
        self.error_kind.clear();
        self.error_message.clear();
        if self.snapshot_generated_at.is_none() {
            self.state = ContainerFilesState::LoadingSnapshot;
            self.entries.clear();
        }
        Some(self.snapshot_generation)
    }

    pub fn apply_snapshot(
        &mut self,
        generation: u64,
        generated_at: DateTime<Utc>,
        overlays: Vec<ContainerMountOverlay>,
    ) -> bool {
        if generation != self.snapshot_generation {
            return false;
        }
        self.snapshot_in_flight = false;
        self.snapshot_generated_at = Some(generated_at);
        self.snapshot_expires_at = Some(
            generated_at + TimeDelta::from_std(CONTAINER_SNAPSHOT_TTL).unwrap_or(TimeDelta::MAX),
        );
        self.snapshot_invalidated = false;
        self.mount_overlays = overlays;
        self.entries.clear();
        self.next_cursor = None;
        self.total_visible = 0;
        self.selected_path = None;
        self.state = ContainerFilesState::LoadingDirectory;
        true
    }

    pub fn apply_snapshot_error(&mut self, generation: u64, kind: &str, message: &str) -> bool {
        if generation != self.snapshot_generation {
            return false;
        }
        self.snapshot_in_flight = false;
        self.error_kind = kind.to_string();
        self.error_message = message.to_string();
        // A failed revalidation does not erase the last point-in-time view.
        if self.snapshot_generated_at.is_some() {
            self.snapshot_invalidated = true;
            self.state = if self.visible_entries().is_empty() {
                ContainerFilesState::Empty
            } else {
                ContainerFilesState::Ready
            };
        } else {
            self.state = ContainerFilesState::Error;
        }
        true
    }

    pub fn invalidate(&mut self) {
        if self.snapshot_generated_at.is_some() {
            self.snapshot_invalidated = true;
        }
        self.snapshot_generation = self.snapshot_generation.wrapping_add(1);
        self.list_generation = self.list_generation.wrapping_add(1);
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.save_generation = self.save_generation.wrapping_add(1);
        self.snapshot_in_flight = false;
    }

    pub fn begin_list(&mut self) -> Option<(u64, ContainerDirectoryQuery)> {
        self.snapshot_generated_at?;
        self.list_generation = self.list_generation.wrapping_add(1);
        self.state = ContainerFilesState::LoadingDirectory;
        self.error_kind.clear();
        self.error_message.clear();
        Some((self.list_generation, self.directory_query(None)))
    }

    pub fn begin_more(&mut self) -> Option<(u64, ContainerDirectoryQuery)> {
        let cursor = self.next_cursor.clone()?;
        self.list_generation = self.list_generation.wrapping_add(1);
        Some((self.list_generation, self.directory_query(Some(cursor))))
    }

    pub fn apply_page(&mut self, generation: u64, page: ContainerDirectoryPage) -> bool {
        if generation != self.list_generation || page.directory != self.current_path {
            return false;
        }
        self.entries = page.entries;
        self.next_cursor = page.next_cursor;
        self.total_visible = page.total_visible;
        self.selected_path = None;
        self.update_content_state();
        true
    }

    pub fn apply_more(&mut self, generation: u64, page: ContainerDirectoryPage) -> bool {
        if generation != self.list_generation || page.directory != self.current_path {
            return false;
        }
        for entry in page.entries {
            if !self
                .entries
                .iter()
                .any(|existing| existing.logical_path == entry.logical_path)
            {
                self.entries.push(entry);
            }
        }
        self.next_cursor = page.next_cursor;
        self.total_visible = page.total_visible;
        self.update_content_state();
        true
    }

    pub fn apply_list_error(&mut self, generation: u64, kind: &str, message: &str) -> bool {
        if generation != self.list_generation {
            return false;
        }
        self.state = ContainerFilesState::Error;
        self.error_kind = kind.to_string();
        self.error_message = message.to_string();
        true
    }

    pub fn navigate_to(&mut self, path: &str, push_history: bool) -> bool {
        let Some(path) = normalize_ui_path(path) else {
            return false;
        };
        if path == self.current_path {
            return false;
        }
        if push_history {
            self.history.push(self.current_path.clone());
        }
        self.current_path = path;
        self.search_query.clear();
        self.selected_path = None;
        self.entries.clear();
        self.next_cursor = None;
        true
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        self.current_path != "/"
    }

    pub fn go_back(&mut self) -> bool {
        let Some(path) = self.history.pop() else {
            return false;
        };
        self.current_path = path;
        self.search_query.clear();
        self.selected_path = None;
        self.entries.clear();
        self.next_cursor = None;
        true
    }

    pub fn go_up(&mut self) -> bool {
        if self.current_path == "/" {
            return false;
        }
        let old = self.current_path.clone();
        self.current_path = parent_path(&old);
        self.history.push(old);
        self.search_query.clear();
        self.selected_path = None;
        self.entries.clear();
        self.next_cursor = None;
        true
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) -> bool {
        if self.show_hidden == show_hidden {
            return false;
        }
        self.show_hidden = show_hidden;
        true
    }

    pub fn set_search(&mut self, query: &str) {
        self.search_query = query.trim().to_string();
        self.update_content_state();
    }

    pub fn toggle_sort(&mut self, column: ContainerFileSortColumn) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
    }

    pub fn select_path(&mut self, path: Option<&str>) {
        self.selected_path = path.and_then(normalize_ui_path);
    }

    pub fn visible_entries(&self) -> Vec<&ContainerFilesystemEntry> {
        let query = self.search_query.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.display_name.to_ascii_lowercase().contains(&query)
                    || entry.logical_path.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn begin_preview(&mut self) -> u64 {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_generation
    }

    pub fn begin_save(&mut self) -> u64 {
        self.save_generation = self.save_generation.wrapping_add(1);
        self.save_generation
    }

    pub fn mount_for_entry(
        &self,
        entry: &ContainerFilesystemEntry,
    ) -> Option<&ContainerMountOverlay> {
        let index = match entry.origin {
            ContainerFilesystemOrigin::MountOverlay { mount_index }
            | ContainerFilesystemOrigin::MountRoute { mount_index }
            | ContainerFilesystemOrigin::ShadowedByMount { mount_index } => mount_index,
            _ => return None,
        };
        self.mount_overlays.get(index)
    }

    pub fn mount_action(&self, entry: &ContainerFilesystemEntry) -> Option<MountAction> {
        if !matches!(entry.origin, ContainerFilesystemOrigin::MountOverlay { .. }) {
            return None;
        }
        let overlay = self.mount_for_entry(entry)?;
        Some(match &overlay.kind {
            ContainerMountOverlayKind::Volume => MountAction::Volume {
                name: overlay.source.clone().unwrap_or_default(),
            },
            ContainerMountOverlayKind::Bind => MountAction::Bind {
                source: overlay.source.clone().unwrap_or_default(),
            },
            ContainerMountOverlayKind::Tmpfs => MountAction::Tmpfs,
            ContainerMountOverlayKind::Other(_) => MountAction::Other,
        })
    }

    pub fn snapshot_status_text(&self, now: DateTime<Utc>) -> String {
        let Some(generated) = self.snapshot_generated_at else {
            return "No snapshot".into();
        };
        let seconds = now.signed_duration_since(generated).num_seconds().max(0);
        let suffix = if self.snapshot_is_fresh_at(now) {
            String::new()
        } else {
            " (not live)".into()
        };
        if seconds < 60 {
            format!("Snapshot updated {seconds} seconds ago{suffix}")
        } else {
            format!("Snapshot updated {} minutes ago{suffix}", seconds / 60)
        }
    }

    pub fn cancel_all(&mut self) {
        self.snapshot_generation = self.snapshot_generation.wrapping_add(1);
        self.list_generation = self.list_generation.wrapping_add(1);
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.save_generation = self.save_generation.wrapping_add(1);
        self.snapshot_in_flight = false;
    }

    fn directory_query(&self, cursor: Option<String>) -> ContainerDirectoryQuery {
        ContainerDirectoryQuery {
            directory: self.current_path.clone(),
            include_hidden: self.show_hidden,
            include_shadowed: false,
            sort: self.sort_column.core(),
            order: if self.sort_descending {
                ContainerDirectorySortOrder::Descending
            } else {
                ContainerDirectorySortOrder::Ascending
            },
            limit: CONTAINER_DIRECTORY_PAGE_SIZE,
            cursor,
        }
    }

    fn update_content_state(&mut self) {
        self.state = if self.visible_entries().is_empty() {
            ContainerFilesState::Empty
        } else {
            ContainerFilesState::Ready
        };
    }

    fn reset_snapshot(&mut self) {
        self.state = ContainerFilesState::Idle;
        self.current_path = "/".into();
        self.history.clear();
        self.search_query.clear();
        self.entries.clear();
        self.mount_overlays.clear();
        self.next_cursor = None;
        self.total_visible = 0;
        self.snapshot_generated_at = None;
        self.snapshot_expires_at = None;
        self.snapshot_invalidated = false;
        self.snapshot_in_flight = false;
        self.error_kind.clear();
        self.error_message.clear();
        self.selected_path = None;
    }
}

fn normalize_ui_path(path: &str) -> Option<String> {
    if !path.starts_with('/') || path.as_bytes().contains(&0) {
        return None;
    }
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            value => components.push(value),
        }
    }
    Some(if components.is_empty() {
        "/".into()
    } else {
        format!("/{}", components.join("/"))
    })
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
        .to_string()
}

pub fn entry_type_name(entry_type: ContainerFilesystemEntryType) -> &'static str {
    match entry_type {
        ContainerFilesystemEntryType::File => "file",
        ContainerFilesystemEntryType::Directory => "directory",
        ContainerFilesystemEntryType::Symlink => "symlink",
        ContainerFilesystemEntryType::Hardlink => "hardlink",
        ContainerFilesystemEntryType::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn entry(path: &str, origin: ContainerFilesystemOrigin) -> ContainerFilesystemEntry {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        ContainerFilesystemEntry {
            raw_path: path.trim_start_matches('/').into(),
            logical_path: path.into(),
            name: name.clone(),
            display_name: name,
            entry_type: ContainerFilesystemEntryType::File,
            size: 10,
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: Some(now()),
            link_target: None,
            origin,
        }
    }

    fn page(
        directory: &str,
        entries: Vec<ContainerFilesystemEntry>,
        next: Option<&str>,
    ) -> ContainerDirectoryPage {
        ContainerDirectoryPage {
            directory: directory.into(),
            total_visible: entries.len() + usize::from(next.is_some()),
            entries,
            next_cursor: next.map(str::to_string),
        }
    }

    fn loaded() -> ContainerFilesControllerState {
        let mut state = ContainerFilesControllerState::default();
        state.select_container("container-a");
        let generation = state.begin_snapshot(now(), false).unwrap();
        assert!(state.apply_snapshot(generation, now(), vec![]));
        state
    }

    #[test]
    fn ttl_is_exactly_ten_seconds_and_expiry_is_not_live() {
        let state = loaded();
        assert!(state.snapshot_is_fresh_at(now() + TimeDelta::seconds(10)));
        assert!(!state.snapshot_is_fresh_at(now() + TimeDelta::seconds(11)));
        assert_eq!(
            state.snapshot_decision(now() + TimeDelta::seconds(11), false),
            SnapshotDecision::RefreshKeepingSnapshot
        );
        assert!(
            state
                .snapshot_status_text(now() + TimeDelta::seconds(11))
                .contains("not live")
        );
    }

    #[test]
    fn fresh_snapshot_reuses_but_manual_refresh_bypasses_ttl() {
        let mut state = loaded();
        assert_eq!(
            state.snapshot_decision(now() + TimeDelta::seconds(2), false),
            SnapshotDecision::Reuse
        );
        assert!(
            state
                .begin_snapshot(now() + TimeDelta::seconds(2), false)
                .is_none()
        );
        assert!(
            state
                .begin_snapshot(now() + TimeDelta::seconds(2), true)
                .is_some()
        );
    }

    #[test]
    fn concurrent_snapshot_requests_share_the_in_flight_operation() {
        let mut state = ContainerFilesControllerState::default();
        state.select_container("container-a");
        let generation = state.begin_snapshot(now(), false).unwrap();
        assert!(state.snapshot_in_flight);
        assert!(state.begin_snapshot(now(), false).is_none());
        assert!(state.begin_snapshot(now(), true).is_none());
        assert_eq!(state.snapshot_generation, generation);

        assert!(state.apply_snapshot(generation, now(), vec![]));
        assert!(!state.snapshot_in_flight);
        assert!(state.begin_snapshot(now(), true).is_some());
    }

    #[test]
    fn selection_change_invalidates_requests_and_drops_old_snapshot() {
        let mut state = loaded();
        let generation = state.snapshot_generation;
        assert!(state.select_container("container-b"));
        assert!(state.snapshot_generation > generation);
        assert!(state.snapshot_generated_at.is_none());
        assert!(state.entries.is_empty());
        assert_eq!(state.container_id, "container-b");
    }

    #[test]
    fn event_invalidation_keeps_visible_snapshot_but_marks_it_stale() {
        let mut state = loaded();
        let list_generation = state.begin_list().unwrap().0;
        state.apply_page(
            list_generation,
            page(
                "/",
                vec![entry("/old", ContainerFilesystemOrigin::RootFilesystem)],
                None,
            ),
        );
        state.invalidate();
        assert_eq!(state.entries.len(), 1);
        assert!(!state.snapshot_is_fresh_at(now()));
        assert_eq!(
            state.snapshot_decision(now(), false),
            SnapshotDecision::RefreshKeepingSnapshot
        );
    }

    #[test]
    fn stale_snapshot_and_directory_results_cannot_overwrite_new_generation() {
        let mut state = loaded();
        let stale_list = state.begin_list().unwrap().0;
        let stale_snapshot = state.begin_snapshot(now(), true).unwrap();
        state.invalidate();
        let fresh_snapshot = state.begin_snapshot(now(), true).unwrap();
        assert!(!state.apply_snapshot(stale_snapshot, now(), vec![]));
        assert!(state.apply_snapshot(fresh_snapshot, now(), vec![]));
        assert!(!state.apply_page(
            stale_list,
            page(
                "/",
                vec![entry("/stale", ContainerFilesystemOrigin::RootFilesystem)],
                None
            )
        ));
    }

    #[test]
    fn invalidation_allows_a_new_snapshot_generation() {
        let mut state = loaded();
        let first = state.begin_snapshot(now(), true).unwrap();
        state.invalidate();
        let second = state.begin_snapshot(now(), true).unwrap();
        assert!(second > first);
        assert!(state.snapshot_in_flight);
        assert!(!state.apply_snapshot(first, now(), vec![]));
        assert!(state.apply_snapshot(second, now(), vec![]));
        assert!(!state.snapshot_in_flight);
    }

    #[test]
    fn pagination_appends_once_and_rejects_wrong_directory() {
        let mut state = loaded();
        let generation = state.begin_list().unwrap().0;
        assert!(state.apply_page(
            generation,
            page(
                "/",
                vec![entry("/a", ContainerFilesystemOrigin::RootFilesystem)],
                Some("cursor")
            )
        ));
        let (more_generation, query) = state.begin_more().unwrap();
        assert_eq!(query.cursor.as_deref(), Some("cursor"));
        assert!(more_generation > generation);
        assert!(state.apply_more(
            more_generation,
            page(
                "/",
                vec![
                    entry("/a", ContainerFilesystemOrigin::RootFilesystem),
                    entry("/b", ContainerFilesystemOrigin::RootFilesystem)
                ],
                None
            )
        ));
        assert_eq!(state.entries.len(), 2);
        assert!(!state.apply_more(generation, page("/other", vec![], None)));
    }

    #[test]
    fn hidden_sort_and_search_are_reflected_in_queries_and_rows() {
        let mut state = loaded();
        state.set_show_hidden(true);
        state.toggle_sort(ContainerFileSortColumn::Size);
        state.toggle_sort(ContainerFileSortColumn::Size);
        let (generation, query) = state.begin_list().unwrap();
        assert!(query.include_hidden);
        assert!(!query.include_shadowed);
        assert_eq!(query.sort, ContainerDirectorySort::Size);
        assert_eq!(query.order, ContainerDirectorySortOrder::Descending);
        state.apply_page(
            generation,
            page(
                "/",
                vec![
                    entry("/Alpha.txt", ContainerFilesystemOrigin::RootFilesystem),
                    entry("/beta.txt", ContainerFilesystemOrigin::RootFilesystem),
                ],
                None,
            ),
        );
        state.set_search("alpha");
        assert_eq!(state.visible_entries().len(), 1);
    }

    #[test]
    fn navigation_back_and_up_are_consistent() {
        let mut state = loaded();
        assert!(state.navigate_to("/usr//local/.", true));
        assert_eq!(state.current_path, "/usr/local");
        assert!(state.go_up());
        assert_eq!(state.current_path, "/usr");
        assert!(state.go_back());
        assert_eq!(state.current_path, "/usr/local");
        assert!(!state.navigate_to("/usr/../secret", true));
    }

    #[test]
    fn mount_overlay_actions_never_treat_shadowed_data_as_mount_content() {
        let overlays = vec![
            ContainerMountOverlay::new(
                ContainerMountOverlayKind::Volume,
                "/data",
                Some("named-data".into()),
                false,
            )
            .unwrap(),
            ContainerMountOverlay::new(
                ContainerMountOverlayKind::Bind,
                "/workspace",
                Some("/home/me/work".into()),
                true,
            )
            .unwrap(),
            ContainerMountOverlay::new(ContainerMountOverlayKind::Tmpfs, "/tmp", None, false)
                .unwrap(),
        ];
        let mut state = loaded();
        state.mount_overlays = overlays;
        assert_eq!(
            state.mount_action(&entry(
                "/data",
                ContainerFilesystemOrigin::MountOverlay { mount_index: 0 }
            )),
            Some(MountAction::Volume {
                name: "named-data".into()
            })
        );
        assert_eq!(
            state.mount_action(&entry(
                "/workspace",
                ContainerFilesystemOrigin::MountOverlay { mount_index: 1 }
            )),
            Some(MountAction::Bind {
                source: "/home/me/work".into()
            })
        );
        assert_eq!(
            state.mount_action(&entry(
                "/tmp",
                ContainerFilesystemOrigin::MountOverlay { mount_index: 2 }
            )),
            Some(MountAction::Tmpfs)
        );
        assert_eq!(
            state.mount_action(&entry(
                "/data/hidden",
                ContainerFilesystemOrigin::ShadowedByMount { mount_index: 0 }
            )),
            None
        );
        assert_eq!(
            state.mount_action(&entry(
                "/",
                ContainerFilesystemOrigin::MountRoute { mount_index: 0 }
            )),
            None
        );
        let query = state.directory_query(None);
        assert!(!query.include_shadowed);
    }

    #[test]
    fn preview_save_and_clear_cancel_old_work() {
        let mut state = loaded();
        let preview = state.begin_preview();
        let save = state.begin_save();
        state.clear_selection();
        assert!(state.preview_generation > preview);
        assert!(state.save_generation > save);
        assert_eq!(state.state, ContainerFilesState::Idle);
    }

    #[test]
    fn failed_background_refresh_preserves_snapshot_without_claiming_live() {
        let mut state = loaded();
        let list_generation = state.begin_list().unwrap().0;
        state.apply_page(
            list_generation,
            page(
                "/",
                vec![entry("/kept", ContainerFilesystemOrigin::RootFilesystem)],
                None,
            ),
        );
        let generation = state.begin_snapshot(now(), true).unwrap();
        assert!(state.apply_snapshot_error(generation, "docker", "refresh failed"));
        assert_eq!(state.entries.len(), 1);
        assert!(state.snapshot_invalidated);
        assert_eq!(state.state, ContainerFilesState::Ready);
    }
}
