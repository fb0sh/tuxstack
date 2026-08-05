//! Pure controller state for read-only volume file browsing.

use std::cmp::Ordering;

use tuxstack_docker_core::{FilesystemEntry, FilesystemEntryType, FilesystemPathToken, VolumePath};

/// Files panel load / session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeFilesState {
    Idle,
    StartingSession,
    Loading,
    Ready,
    Empty,
    Error,
    HelperImageRequired,
}

impl VolumeFilesState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::StartingSession => "starting",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::Error => "error",
            Self::HelperImageRequired => "helper_image_required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeFileSortColumn {
    Name,
    Modified,
    Size,
    Kind,
}

impl VolumeFileSortColumn {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Kind => "kind",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "modified" | "date" | "mtime" => Self::Modified,
            "size" => Self::Size,
            "kind" | "type" => Self::Kind,
            _ => Self::Name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VolumeFilesControllerState {
    pub state: VolumeFilesState,
    pub error_kind: String,
    pub error_message: String,
    pub active: bool,
    pub volume_name: String,
    pub current_path: VolumePath,
    pub history: Vec<VolumePath>,
    pub show_hidden: bool,
    pub search_query: String,
    pub sort_column: VolumeFileSortColumn,
    pub sort_descending: bool,
    pub directories_first: bool,
    pub selected_path: Option<VolumePath>,
    pub entries: Vec<FilesystemEntry>,
    pub truncated: bool,
    pub next_cursor: Option<String>,
    pub current_path_token: FilesystemPathToken,
    pub session_generation: u64,
    pub list_generation: u64,
}

impl Default for VolumeFilesControllerState {
    fn default() -> Self {
        Self {
            state: VolumeFilesState::Idle,
            error_kind: String::new(),
            error_message: String::new(),
            active: false,
            volume_name: String::new(),
            current_path: VolumePath::root(),
            history: Vec::new(),
            show_hidden: false,
            search_query: String::new(),
            sort_column: VolumeFileSortColumn::Name,
            sort_descending: false,
            directories_first: true,
            selected_path: None,
            entries: Vec::new(),
            truncated: false,
            next_cursor: None,
            current_path_token: FilesystemPathToken::root_token(),
            session_generation: 0,
            list_generation: 0,
        }
    }
}

impl VolumeFilesControllerState {
    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        !self.current_path.is_root()
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            // Leaving Files: controller keeps volume name for re-entry but
            // session teardown is owned by the bridge.
            if !matches!(
                self.state,
                VolumeFilesState::Error | VolumeFilesState::HelperImageRequired
            ) {
                // Keep last listing visually until session is closed by bridge.
            }
        }
    }

    pub fn begin_volume(&mut self, volume_name: &str) -> u64 {
        let same = self.volume_name == volume_name;
        self.session_generation = self.session_generation.saturating_add(1);
        self.list_generation = self.list_generation.saturating_add(1);
        self.volume_name = volume_name.to_string();
        self.current_path = VolumePath::root();
        self.current_path_token = FilesystemPathToken::root_token();
        self.history.clear();
        self.selected_path = None;
        self.entries.clear();
        self.truncated = false;
        self.next_cursor = None;
        self.error_kind.clear();
        self.error_message.clear();
        self.search_query.clear();
        if !same {
            // keep sort preferences across volumes
        }
        self.state = VolumeFilesState::StartingSession;
        self.session_generation
    }

    pub fn clear_volume(&mut self) {
        self.session_generation = self.session_generation.saturating_add(1);
        self.list_generation = self.list_generation.saturating_add(1);
        self.volume_name.clear();
        self.current_path = VolumePath::root();
        self.current_path_token = FilesystemPathToken::root_token();
        self.history.clear();
        self.selected_path = None;
        self.entries.clear();
        self.truncated = false;
        self.next_cursor = None;
        self.error_kind.clear();
        self.error_message.clear();
        self.state = VolumeFilesState::Idle;
    }

    pub fn begin_list(&mut self) -> u64 {
        self.list_generation = self.list_generation.saturating_add(1);
        if self.entries.is_empty() {
            self.state = VolumeFilesState::Loading;
        } else {
            // Keep previous rows while loading a new directory.
            self.state = VolumeFilesState::Loading;
        }
        self.error_kind.clear();
        self.error_message.clear();
        self.list_generation
    }

    pub fn apply_list(
        &mut self,
        generation: u64,
        path: VolumePath,
        path_token: FilesystemPathToken,
        result: tuxstack_docker_core::ListDirectoryResult,
    ) -> bool {
        if generation != self.list_generation {
            return false;
        }
        self.current_path = path;
        self.current_path_token = path_token;
        self.truncated = result.truncated;
        self.next_cursor = result.next_cursor;
        self.selected_path = None;
        let mut entries = result.entries;
        sort_entries(
            &mut entries,
            self.sort_column,
            self.sort_descending,
            self.directories_first,
        );
        self.entries = entries;
        self.state = if self.visible_entries().is_empty() {
            VolumeFilesState::Empty
        } else {
            VolumeFilesState::Ready
        };
        true
    }

    pub fn apply_error(&mut self, generation: u64, kind: &str, message: &str) -> bool {
        if generation != self.list_generation && generation != self.session_generation {
            // Accept session-start errors against either generation.
            if generation != self.session_generation {
                return false;
            }
        }
        if kind == "helper_image_missing" {
            self.state = VolumeFilesState::HelperImageRequired;
        } else {
            self.state = VolumeFilesState::Error;
        }
        self.error_kind = kind.to_string();
        self.error_message = message.to_string();
        self.entries.clear();
        true
    }

    pub fn navigate_to(&mut self, path: VolumePath, push_history: bool) {
        if push_history && path != self.current_path {
            self.history.push(self.current_path.clone());
        }
        self.current_path = path;
        self.search_query.clear();
        self.selected_path = None;
    }

    pub fn go_back(&mut self) -> Option<VolumePath> {
        let previous = self.history.pop()?;
        self.current_path = previous.clone();
        self.search_query.clear();
        self.selected_path = None;
        Some(previous)
    }

    pub fn go_up(&mut self) -> Option<VolumePath> {
        let parent = self.current_path.parent()?;
        self.history.push(self.current_path.clone());
        self.current_path = parent.clone();
        self.search_query.clear();
        self.selected_path = None;
        Some(parent)
    }

    pub fn set_search_query(&mut self, query: &str) {
        self.search_query = query.to_string();
        if self.state == VolumeFilesState::Ready || self.state == VolumeFilesState::Empty {
            self.state = if self.visible_entries().is_empty() {
                VolumeFilesState::Empty
            } else {
                VolumeFilesState::Ready
            };
        }
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
    }

    pub fn set_sort(&mut self, column: VolumeFileSortColumn, descending: bool) {
        self.sort_column = column;
        self.sort_descending = descending;
        sort_entries(
            &mut self.entries,
            self.sort_column,
            self.sort_descending,
            self.directories_first,
        );
    }

    pub fn toggle_sort(&mut self, column: VolumeFileSortColumn) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
        sort_entries(
            &mut self.entries,
            self.sort_column,
            self.sort_descending,
            self.directories_first,
        );
    }

    pub fn select_path(&mut self, path: Option<VolumePath>) {
        self.selected_path = path;
    }

    pub fn visible_entries(&self) -> Vec<&FilesystemEntry> {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                if !self.show_hidden && entry.hidden {
                    return false;
                }
                if query.is_empty() {
                    return true;
                }
                entry.display_name.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn breadcrumb_components(&self) -> Vec<(String, VolumePath)> {
        let mut items = vec![(
            if self.volume_name.is_empty() {
                "/".into()
            } else {
                self.volume_name.clone()
            },
            VolumePath::root(),
        )];
        let mut acc = VolumePath::root();
        for component in self.current_path.components() {
            acc = acc.join_name(component).unwrap_or(acc);
            items.push((component.clone(), acc.clone()));
        }
        items
    }
}

pub fn sort_entries(
    entries: &mut [FilesystemEntry],
    column: VolumeFileSortColumn,
    descending: bool,
    directories_first: bool,
) {
    entries.sort_by(|left, right| {
        if directories_first {
            let left_dir = left.entry_type.is_directory();
            let right_dir = right.entry_type.is_directory();
            match (left_dir, right_dir) {
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {}
            }
        }
        let ordering = match column {
            VolumeFileSortColumn::Name => cmp_name(left, right),
            VolumeFileSortColumn::Modified => cmp_modified(left, right),
            VolumeFileSortColumn::Size => cmp_size(left, right),
            VolumeFileSortColumn::Kind => cmp_kind(left, right),
        };
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn cmp_name(left: &FilesystemEntry, right: &FilesystemEntry) -> Ordering {
    let primary = left
        .display_name
        .to_ascii_lowercase()
        .cmp(&right.display_name.to_ascii_lowercase());
    if primary == Ordering::Equal {
        left.name_raw.cmp(&right.name_raw)
    } else {
        primary
    }
}

fn cmp_modified(left: &FilesystemEntry, right: &FilesystemEntry) -> Ordering {
    match (left.modified_at, right.modified_at) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => cmp_name(left, right),
    }
}

fn cmp_size(left: &FilesystemEntry, right: &FilesystemEntry) -> Ordering {
    match (left.size_bytes, right.size_bytes) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => cmp_name(left, right),
    }
}

fn cmp_kind(left: &FilesystemEntry, right: &FilesystemEntry) -> Ordering {
    kind_rank(left.entry_type)
        .cmp(&kind_rank(right.entry_type))
        .then_with(|| cmp_name(left, right))
}

fn kind_rank(entry_type: FilesystemEntryType) -> u8 {
    match entry_type {
        FilesystemEntryType::Directory => 0,
        FilesystemEntryType::RegularFile => 1,
        FilesystemEntryType::SymbolicLink => 2,
        FilesystemEntryType::Socket => 3,
        FilesystemEntryType::Fifo => 4,
        FilesystemEntryType::BlockDevice => 5,
        FilesystemEntryType::CharacterDevice => 6,
        FilesystemEntryType::Unknown => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(name: &str, dir: bool, size: Option<u64>) -> FilesystemEntry {
        FilesystemEntry {
            name_raw: name.as_bytes().to_vec(),
            display_name: name.into(),
            path_token: FilesystemPathToken::from_relative(name).unwrap(),
            entry_type: if dir {
                FilesystemEntryType::Directory
            } else {
                FilesystemEntryType::RegularFile
            },
            size_bytes: size,
            modified_at: Some(Utc::now()),
            mode: Some(0o644),
            uid: Some(0),
            gid: Some(0),
            symlink_target_raw: None,
            symlink_target_display: None,
            readable: true,
            hidden: name.starts_with('.'),
        }
    }

    #[test]
    fn directories_first_and_name_sort() {
        let mut entries = vec![
            entry("b.txt", false, Some(2)),
            entry("a", true, None),
            entry("c", true, None),
            entry("a.txt", false, Some(1)),
        ];
        sort_entries(&mut entries, VolumeFileSortColumn::Name, false, true);
        let names: Vec<_> = entries.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["a", "c", "a.txt", "b.txt"]);
    }

    #[test]
    fn search_filters_current_directory_only() {
        let mut state = VolumeFilesControllerState {
            entries: vec![
                entry("readme.txt", false, Some(1)),
                entry("data", true, None),
            ],
            state: VolumeFilesState::Ready,
            ..Default::default()
        };
        state.set_search_query("read");
        assert_eq!(state.visible_entries().len(), 1);
        assert_eq!(state.visible_entries()[0].display_name, "readme.txt");
    }

    #[test]
    fn navigation_history_back_and_up() {
        let mut state = VolumeFilesControllerState::default();
        state.begin_volume("vol");
        state.navigate_to(VolumePath::parse("/a").unwrap(), true);
        state.navigate_to(VolumePath::parse("/a/b").unwrap(), true);
        assert!(state.can_go_back());
        assert_eq!(state.go_back().unwrap().display(), "/a");
        assert_eq!(state.go_up().unwrap().display(), "/");
        assert!(!state.can_go_up());
    }

    fn list_result(entries: Vec<FilesystemEntry>) -> tuxstack_docker_core::ListDirectoryResult {
        tuxstack_docker_core::ListDirectoryResult {
            entries,
            truncated: false,
            next_cursor: None,
        }
    }

    #[test]
    fn generation_rejects_stale_list() {
        let mut state = VolumeFilesControllerState::default();
        let generation = state.begin_list();
        assert!(!state.apply_list(
            generation - 1,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![])
        ));
        assert!(state.apply_list(
            generation,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![])
        ));
        assert_eq!(state.state, VolumeFilesState::Empty);
    }

    #[test]
    fn begin_volume_starts_session_and_clears_previous_rows() {
        let mut state = VolumeFilesControllerState::default();
        let _ = state.begin_volume("alpha");
        let list_gen = state.begin_list();
        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("old.txt", false, Some(1))])
        ));
        assert_eq!(state.state, VolumeFilesState::Ready);
        assert_eq!(state.entries.len(), 1);

        let session_gen = state.begin_volume("beta");
        assert!(session_gen > 0);
        assert_eq!(state.volume_name, "beta");
        assert_eq!(state.state, VolumeFilesState::StartingSession);
        assert!(state.entries.is_empty());
        assert!(state.current_path.is_root());
        assert!(state.history.is_empty());
    }

    #[test]
    fn starting_session_to_loading_to_ready() {
        let mut state = VolumeFilesControllerState::default();
        let session_gen = state.begin_volume("data");
        assert_eq!(state.state, VolumeFilesState::StartingSession);
        assert!(session_gen > 0);

        let list_gen = state.begin_list();
        assert_eq!(state.state, VolumeFilesState::Loading);

        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("file.txt", false, Some(3))])
        ));
        assert_eq!(state.state, VolumeFilesState::Ready);
        assert_eq!(state.visible_entries().len(), 1);
    }

    #[test]
    fn loading_to_empty_and_error() {
        let mut state = VolumeFilesControllerState::default();
        let _ = state.begin_volume("empty-vol");
        let list_gen = state.begin_list();
        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![])
        ));
        assert_eq!(state.state, VolumeFilesState::Empty);

        let list_gen = state.begin_list();
        assert!(state.apply_error(list_gen, "timeout", "Operation timed out."));
        assert_eq!(state.state, VolumeFilesState::Error);
        assert_eq!(state.error_message, "Operation timed out.");
        assert!(state.entries.is_empty());

        let list_gen = state.begin_list();
        assert!(state.apply_error(list_gen, "helper_image_missing", "missing"));
        assert_eq!(state.state, VolumeFilesState::HelperImageRequired);
    }

    #[test]
    fn clear_volume_returns_to_idle() {
        let mut state = VolumeFilesControllerState::default();
        let _ = state.begin_volume("vol");
        state.clear_volume();
        assert_eq!(state.state, VolumeFilesState::Idle);
        assert!(state.volume_name.is_empty());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn stale_list_cannot_overwrite_newer_volume() {
        let mut state = VolumeFilesControllerState::default();
        let _ = state.begin_volume("one");
        let stale = state.begin_list();
        let _ = state.begin_volume("two");
        let fresh = state.begin_list();
        assert!(!state.apply_list(
            stale,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("stale.txt", false, Some(1))])
        ));
        assert!(state.apply_list(
            fresh,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("fresh.txt", false, Some(2))])
        ));
        assert_eq!(state.volume_name, "two");
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].display_name, "fresh.txt");
    }
}
