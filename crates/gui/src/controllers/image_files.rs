//! Pure controller state for read-only image file browsing.

use tuxstack_docker_core::{FilesystemEntry, FilesystemEntryType, FilesystemPathToken, VolumePath};

use crate::controllers::volume_files::{VolumeFileSortColumn, sort_entries};

/// Files panel load / session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFilesState {
    Idle,
    StartingSession,
    Loading,
    Ready,
    Empty,
    Error,
    /// The selected image cannot run a helper container (scratch,
    /// distroless, wrong architecture, windows image).
    Unsupported,
}

impl ImageFilesState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::StartingSession => "starting",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::Error => "error",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageFilesControllerState {
    pub state: ImageFilesState,
    pub error_kind: String,
    pub error_message: String,
    pub active: bool,
    pub image_id: String,
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

impl Default for ImageFilesControllerState {
    fn default() -> Self {
        Self {
            state: ImageFilesState::Idle,
            error_kind: String::new(),
            error_message: String::new(),
            active: false,
            image_id: String::new(),
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
            current_path_token: FilesystemPathToken(String::new()),
            session_generation: 0,
            list_generation: 0,
        }
    }
}

impl ImageFilesControllerState {
    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        !self.current_path.is_root()
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn begin_image(&mut self, image_id: &str) -> u64 {
        self.session_generation = self.session_generation.saturating_add(1);
        self.list_generation = self.list_generation.saturating_add(1);
        self.image_id = image_id.to_string();
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
        self.state = ImageFilesState::StartingSession;
        self.session_generation
    }

    pub fn clear_image(&mut self) {
        self.session_generation = self.session_generation.saturating_add(1);
        self.list_generation = self.list_generation.saturating_add(1);
        self.image_id.clear();
        self.current_path = VolumePath::root();
        self.current_path_token = FilesystemPathToken::root_token();
        self.history.clear();
        self.selected_path = None;
        self.entries.clear();
        self.truncated = false;
        self.next_cursor = None;
        self.error_kind.clear();
        self.error_message.clear();
        self.state = ImageFilesState::Idle;
    }

    pub fn begin_list(&mut self) -> u64 {
        self.list_generation = self.list_generation.saturating_add(1);
        self.state = ImageFilesState::Loading;
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
            ImageFilesState::Empty
        } else {
            ImageFilesState::Ready
        };
        true
    }

    pub fn apply_more(
        &mut self,
        generation: u64,
        result: tuxstack_docker_core::ListDirectoryResult,
    ) -> bool {
        if generation != self.list_generation {
            return false;
        }
        self.truncated = result.truncated;
        self.next_cursor = result.next_cursor;
        let mut new_entries = result.entries;
        sort_entries(
            &mut new_entries,
            self.sort_column,
            self.sort_descending,
            self.directories_first,
        );
        self.entries.extend(new_entries);
        true
    }

    pub fn apply_error(&mut self, generation: u64, kind: &str, message: &str) -> bool {
        if generation != self.list_generation && generation != self.session_generation {
            return false;
        }
        if kind == "unsupported" {
            self.state = ImageFilesState::Unsupported;
        } else {
            self.state = ImageFilesState::Error;
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
        if self.state == ImageFilesState::Ready || self.state == ImageFilesState::Empty {
            self.state = if self.visible_entries().is_empty() {
                ImageFilesState::Empty
            } else {
                ImageFilesState::Ready
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
            if self.image_id.is_empty() {
                "/".into()
            } else {
                self.image_id.clone()
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

    fn list_result(entries: Vec<FilesystemEntry>) -> tuxstack_docker_core::ListDirectoryResult {
        tuxstack_docker_core::ListDirectoryResult {
            entries,
            truncated: false,
            next_cursor: None,
        }
    }

    #[test]
    fn begin_image_starts_session_and_clears_previous_rows() {
        let mut state = ImageFilesControllerState::default();
        let _ = state.begin_image("sha256:one");
        let list_gen = state.begin_list();
        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("old.txt", false, Some(1))])
        ));
        assert_eq!(state.state, ImageFilesState::Ready);
        assert_eq!(state.entries.len(), 1);

        let session_gen = state.begin_image("sha256:two");
        assert!(session_gen > 0);
        assert_eq!(state.image_id, "sha256:two");
        assert_eq!(state.state, ImageFilesState::StartingSession);
        assert!(state.entries.is_empty());
        assert!(state.current_path.is_root());
        assert!(state.history.is_empty());
    }

    #[test]
    fn starting_session_to_loading_to_ready() {
        let mut state = ImageFilesControllerState::default();
        let session_gen = state.begin_image("sha256:data");
        assert_eq!(state.state, ImageFilesState::StartingSession);
        assert!(session_gen > 0);

        let list_gen = state.begin_list();
        assert_eq!(state.state, ImageFilesState::Loading);

        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![entry("file.txt", false, Some(3))])
        ));
        assert_eq!(state.state, ImageFilesState::Ready);
        assert_eq!(state.visible_entries().len(), 1);
    }

    #[test]
    fn loading_to_empty_error_and_unsupported() {
        let mut state = ImageFilesControllerState::default();
        let _ = state.begin_image("sha256:empty");
        let list_gen = state.begin_list();
        assert!(state.apply_list(
            list_gen,
            VolumePath::root(),
            FilesystemPathToken::root_token(),
            list_result(vec![])
        ));
        assert_eq!(state.state, ImageFilesState::Empty);

        let list_gen = state.begin_list();
        assert!(state.apply_error(list_gen, "timeout", "Operation timed out."));
        assert_eq!(state.state, ImageFilesState::Error);
        assert!(state.entries.is_empty());

        let list_gen = state.begin_list();
        assert!(state.apply_error(list_gen, "unsupported", "no shell"));
        assert_eq!(state.state, ImageFilesState::Unsupported);
        assert_eq!(state.error_message, "no shell");
    }

    #[test]
    fn clear_image_returns_to_idle() {
        let mut state = ImageFilesControllerState::default();
        let _ = state.begin_image("sha256:vol");
        state.clear_image();
        assert_eq!(state.state, ImageFilesState::Idle);
        assert!(state.image_id.is_empty());
        assert!(state.entries.is_empty());
    }

    #[test]
    fn navigation_history_back_and_up() {
        let mut state = ImageFilesControllerState::default();
        state.begin_image("sha256:img");
        state.navigate_to(VolumePath::parse("/usr").unwrap(), true);
        state.navigate_to(VolumePath::parse("/usr/bin").unwrap(), true);
        assert!(state.can_go_back());
        assert_eq!(state.go_back().unwrap().display(), "/usr");
        assert_eq!(state.go_up().unwrap().display(), "/");
        assert!(!state.can_go_up());
    }

    #[test]
    fn stale_list_cannot_overwrite_newer_image() {
        let mut state = ImageFilesControllerState::default();
        let _ = state.begin_image("sha256:one");
        let stale = state.begin_list();
        let _ = state.begin_image("sha256:two");
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
        assert_eq!(state.image_id, "sha256:two");
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].display_name, "fresh.txt");
    }

    #[test]
    fn search_filters_current_directory_only() {
        let mut state = ImageFilesControllerState {
            entries: vec![entry("readme.txt", false, Some(1)), entry("data", true, None)],
            state: ImageFilesState::Ready,
            ..Default::default()
        };
        state.set_search_query("read");
        assert_eq!(state.visible_entries().len(), 1);
        assert_eq!(state.visible_entries()[0].display_name, "readme.txt");
    }
}
