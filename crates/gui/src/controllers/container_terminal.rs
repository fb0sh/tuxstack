//! Lifecycle and terminal-emulation state for a real Docker exec terminal.
//!
//! Docker TTY bytes are interpreted here with `vt100`; QML receives only
//! rendered screen rows and cursor metadata, never ANSI escape sequences.

use tuxstack_docker_core::{ContainerTerminalError, ContainerTerminalState};

pub const DEFAULT_TERMINAL_ROWS: u16 = 24;
pub const DEFAULT_TERMINAL_COLUMNS: u16 = 80;
pub const TERMINAL_SCROLLBACK_ROWS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSelectionAction {
    None,
    Connect { generation: u64 },
    Close { generation: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingResize {
    pub generation: u64,
    pub rows: u16,
    pub columns: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalScreenSnapshot {
    pub rows: Vec<String>,
    pub row_count: u16,
    pub column_count: u16,
    pub cursor_row: u16,
    pub cursor_column: u16,
    pub cursor_visible: bool,
    pub alternate_screen: bool,
    pub scrollback: usize,
}

/// Generation-guarded VT100 parser. Keeping the guard beside the parser makes
/// it impossible for a queued chunk from an old Docker exec to paint a newly
/// selected container's screen.
pub struct ContainerTerminalRenderer {
    generation: u64,
    parser: vt100::Parser,
}

impl Default for ContainerTerminalRenderer {
    fn default() -> Self {
        Self::new(0, DEFAULT_TERMINAL_ROWS, DEFAULT_TERMINAL_COLUMNS)
    }
}

impl ContainerTerminalRenderer {
    pub fn new(generation: u64, rows: u16, columns: u16) -> Self {
        Self {
            generation,
            parser: vt100::Parser::new(rows.max(1), columns.max(1), TERMINAL_SCROLLBACK_ROWS),
        }
    }

    pub fn reset(&mut self, generation: u64, rows: u16, columns: u16) {
        *self = Self::new(generation, rows, columns);
    }

    pub fn process(&mut self, generation: u64, bytes: &[u8]) -> bool {
        if generation != self.generation || bytes.is_empty() {
            return false;
        }
        self.parser.process(bytes);
        true
    }

    pub fn resize(&mut self, generation: u64, rows: u16, columns: u16) -> bool {
        if generation != self.generation || rows == 0 || columns == 0 {
            return false;
        }
        if self.parser.screen().size() == (rows, columns) {
            return false;
        }
        self.parser.screen_mut().set_size(rows, columns);
        true
    }

    pub fn scroll_lines(&mut self, generation: u64, lines: i32) -> bool {
        if generation != self.generation || lines == 0 {
            return false;
        }
        let current = self.parser.screen().scrollback();
        let next = if lines > 0 {
            current.saturating_add(lines as usize)
        } else {
            current.saturating_sub(lines.unsigned_abs() as usize)
        };
        self.parser.screen_mut().set_scrollback(next);
        self.parser.screen().scrollback() != current
    }

    pub fn snap_to_bottom(&mut self, generation: u64) -> bool {
        if generation != self.generation || self.parser.screen().scrollback() == 0 {
            return false;
        }
        self.parser.screen_mut().set_scrollback(0);
        true
    }

    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn snapshot(&self) -> TerminalScreenSnapshot {
        let screen = self.parser.screen();
        let (row_count, column_count) = screen.size();
        let (cursor_row, cursor_column) = screen.cursor_position();
        TerminalScreenSnapshot {
            rows: screen.rows(0, column_count).collect(),
            row_count,
            column_count,
            cursor_row,
            cursor_column,
            cursor_visible: !screen.hide_cursor() && screen.scrollback() == 0,
            alternate_screen: screen.alternate_screen(),
            scrollback: screen.scrollback(),
        }
    }
}

/// Translate symbolic QML key names to terminal input. Cursor keys respect the
/// VT100 application-cursor mode selected by the program inside the container.
pub fn terminal_key_bytes(key: &str, application_cursor: bool) -> Option<Vec<u8>> {
    let bytes: &[u8] = match key.to_ascii_lowercase().as_str() {
        "enter" => b"\r",
        "backspace" => b"\x7f",
        "tab" => b"\t",
        "escape" => b"\x1b",
        "up" if application_cursor => b"\x1bOA",
        "down" if application_cursor => b"\x1bOB",
        "right" if application_cursor => b"\x1bOC",
        "left" if application_cursor => b"\x1bOD",
        "home" if application_cursor => b"\x1bOH",
        "end" if application_cursor => b"\x1bOF",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        "insert" => b"\x1b[2~",
        key if key.len() == 6 && key.starts_with("ctrl+") => {
            let letter = key.as_bytes()[5];
            if letter.is_ascii_lowercase() {
                return Some(vec![letter - b'a' + 1]);
            }
            return None;
        }
        _ => return None,
    };
    Some(bytes.to_vec())
}

pub fn terminal_paste_bytes(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if bracketed_paste {
        let mut bytes = Vec::with_capacity(text.len() + 12);
        bytes.extend_from_slice(b"\x1b[200~");
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        text.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone)]
pub struct ContainerTerminalControllerState {
    pub state: ContainerTerminalState,
    pub container_id: String,
    pub selected_running: bool,
    pub active: bool,
    pub shell: String,
    pub error_kind: String,
    pub error_message: String,
    pub exit_code: Option<i64>,
    pub rows: u16,
    pub columns: u16,
    pub generation: u64,
    pub input_generation: u64,
    pub resize_generation: u64,
    pub pending_resize: Option<PendingResize>,
}

impl Default for ContainerTerminalControllerState {
    fn default() -> Self {
        Self {
            state: ContainerTerminalState::Idle,
            container_id: String::new(),
            selected_running: false,
            active: false,
            shell: String::new(),
            error_kind: String::new(),
            error_message: String::new(),
            exit_code: None,
            rows: DEFAULT_TERMINAL_ROWS,
            columns: DEFAULT_TERMINAL_COLUMNS,
            generation: 0,
            input_generation: 0,
            resize_generation: 0,
            pending_resize: None,
        }
    }
}

impl ContainerTerminalControllerState {
    pub fn state_name(&self) -> &'static str {
        match self.state {
            ContainerTerminalState::Idle => "idle",
            ContainerTerminalState::Connecting => "connecting",
            ContainerTerminalState::Ready => "ready",
            ContainerTerminalState::Exited => "exited",
            ContainerTerminalState::Error => "error",
        }
    }

    pub fn set_active(&mut self, active: bool) -> TerminalSelectionAction {
        self.active = active;
        if active && self.selected_running && self.state == ContainerTerminalState::Idle {
            return self.begin_connect();
        }
        // Leaving the tab retains an existing session; it never creates one.
        TerminalSelectionAction::None
    }

    pub fn select_container(
        &mut self,
        container_id: &str,
        running: bool,
    ) -> TerminalSelectionAction {
        let container_id = container_id.trim();
        if self.container_id == container_id && self.selected_running == running {
            return TerminalSelectionAction::None;
        }
        let had_session = matches!(
            self.state,
            ContainerTerminalState::Connecting | ContainerTerminalState::Ready
        );
        self.generation = self.generation.wrapping_add(1);
        self.input_generation = self.input_generation.wrapping_add(1);
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.container_id = container_id.to_string();
        self.selected_running = running;
        self.state = ContainerTerminalState::Idle;
        self.shell.clear();
        self.error_kind.clear();
        self.error_message.clear();
        self.exit_code = None;
        self.pending_resize = None;

        if self.active && running && !container_id.is_empty() {
            self.begin_connect()
        } else if had_session {
            TerminalSelectionAction::Close {
                generation: self.generation,
            }
        } else {
            TerminalSelectionAction::None
        }
    }

    pub fn clear_selection(&mut self) -> TerminalSelectionAction {
        self.select_container("", false)
    }

    pub fn begin_connect(&mut self) -> TerminalSelectionAction {
        if !self.active || !self.selected_running || self.container_id.is_empty() {
            return TerminalSelectionAction::None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.input_generation = self.input_generation.wrapping_add(1);
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.state = ContainerTerminalState::Connecting;
        self.shell.clear();
        self.error_kind.clear();
        self.error_message.clear();
        self.exit_code = None;
        self.pending_resize = None;
        TerminalSelectionAction::Connect {
            generation: self.generation,
        }
    }

    pub fn apply_connected(&mut self, generation: u64, shell: &str) -> bool {
        if generation != self.generation || self.state != ContainerTerminalState::Connecting {
            return false;
        }
        self.state = ContainerTerminalState::Ready;
        self.shell = shell.to_string();
        true
    }

    pub fn apply_connect_error(&mut self, generation: u64, error: ContainerTerminalError) -> bool {
        if generation != self.generation || self.state != ContainerTerminalState::Connecting {
            return false;
        }
        self.state = match error {
            ContainerTerminalError::NotRunning | ContainerTerminalError::Paused => {
                ContainerTerminalState::Exited
            }
            _ => ContainerTerminalState::Error,
        };
        let (kind, message) = friendly_error(error);
        self.error_kind = kind.into();
        self.error_message = message.into();
        true
    }

    pub fn accept_input(&mut self) -> Option<u64> {
        if self.state != ContainerTerminalState::Ready {
            return None;
        }
        self.input_generation = self.input_generation.wrapping_add(1);
        Some(self.input_generation)
    }

    /// Store the latest size only. The bridge owns a short timer and calls
    /// `take_pending_resize`, so rapid geometry changes collapse to one Docker
    /// resize call.
    pub fn request_resize(&mut self, rows: u16, columns: u16) -> Option<u64> {
        if self.state != ContainerTerminalState::Ready || rows == 0 || columns == 0 {
            return None;
        }
        if self.rows == rows && self.columns == columns && self.pending_resize.is_none() {
            return None;
        }
        self.rows = rows;
        self.columns = columns;
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.pending_resize = Some(PendingResize {
            generation: self.resize_generation,
            rows,
            columns,
        });
        Some(self.resize_generation)
    }

    pub fn take_pending_resize(&mut self, generation: u64) -> Option<PendingResize> {
        if self
            .pending_resize
            .as_ref()
            .is_some_and(|pending| pending.generation == generation)
        {
            self.pending_resize.take()
        } else {
            None
        }
    }

    pub fn apply_exited(&mut self, generation: u64, exit_code: Option<i64>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = ContainerTerminalState::Exited;
        self.exit_code = exit_code;
        self.input_generation = self.input_generation.wrapping_add(1);
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.pending_resize = None;
        true
    }

    pub fn apply_stream_error(&mut self, generation: u64, error: ContainerTerminalError) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = ContainerTerminalState::Error;
        let (kind, message) = friendly_error(error);
        self.error_kind = kind.into();
        self.error_message = message.into();
        self.pending_resize = None;
        true
    }

    /// Docker stop/die/restart events invalidate the session immediately.
    pub fn container_event_invalidated(&mut self, container_id: &str) -> TerminalSelectionAction {
        if self.container_id != container_id {
            return TerminalSelectionAction::None;
        }
        self.selected_running = false;
        self.generation = self.generation.wrapping_add(1);
        self.input_generation = self.input_generation.wrapping_add(1);
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.pending_resize = None;
        self.state = ContainerTerminalState::Exited;
        self.error_kind.clear();
        self.error_message.clear();
        TerminalSelectionAction::Close {
            generation: self.generation,
        }
    }

    pub fn close(&mut self) -> TerminalSelectionAction {
        let had_session = matches!(
            self.state,
            ContainerTerminalState::Connecting | ContainerTerminalState::Ready
        );
        self.generation = self.generation.wrapping_add(1);
        self.input_generation = self.input_generation.wrapping_add(1);
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.pending_resize = None;
        self.state = if self.container_id.is_empty() {
            ContainerTerminalState::Idle
        } else {
            ContainerTerminalState::Exited
        };
        if had_session {
            TerminalSelectionAction::Close {
                generation: self.generation,
            }
        } else {
            TerminalSelectionAction::None
        }
    }
}

pub fn friendly_error(error: ContainerTerminalError) -> (&'static str, &'static str) {
    match error {
        ContainerTerminalError::NotRunning => {
            ("not_running", "Start the container to open a terminal.")
        }
        ContainerTerminalError::Paused => ("paused", "Resume the container to open a terminal."),
        ContainerTerminalError::ShellNotFound => (
            "shell_not_found",
            "No supported shell was found in this container.",
        ),
        ContainerTerminalError::CreateFailed => (
            "create_failed",
            "Docker could not create the terminal session.",
        ),
        ContainerTerminalError::StartFailed => (
            "start_failed",
            "Docker could not start the terminal session.",
        ),
        ContainerTerminalError::Disconnected => {
            ("disconnected", "The terminal connection was lost.")
        }
        ContainerTerminalError::ResizeFailed => {
            ("resize_failed", "Docker could not resize the terminal.")
        }
        ContainerTerminalError::Timeout => ("timeout", "The terminal operation timed out."),
        ContainerTerminalError::Cancelled => ("cancelled", "The terminal operation was cancelled."),
        ContainerTerminalError::InvalidOptions => {
            ("invalid_options", "The terminal options are invalid.")
        }
        ContainerTerminalError::Permission => (
            "permission",
            "Permission to use the Docker terminal was denied.",
        ),
        ContainerTerminalError::DockerUnavailable => {
            ("docker_unavailable", "Docker Engine is unavailable.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ContainerTerminalControllerState {
        let mut state = ContainerTerminalControllerState::default();
        state.set_active(true);
        let TerminalSelectionAction::Connect { generation } =
            state.select_container("container-a", true)
        else {
            panic!()
        };
        assert!(state.apply_connected(generation, "/bin/sh"));
        state
    }

    #[test]
    fn only_active_running_selection_connects() {
        let mut state = ContainerTerminalControllerState::default();
        assert_eq!(
            state.select_container("stopped", false),
            TerminalSelectionAction::None
        );
        assert_eq!(state.set_active(true), TerminalSelectionAction::None);
        let action = state.select_container("running", true);
        assert!(matches!(action, TerminalSelectionAction::Connect { .. }));
        assert_eq!(state.state, ContainerTerminalState::Connecting);
    }

    #[test]
    fn explicit_idle_connecting_ready_state_flow() {
        let mut state = ContainerTerminalControllerState::default();
        assert_eq!(state.state_name(), "idle");
        state.set_active(true);
        let TerminalSelectionAction::Connect { generation } = state.select_container("a", true)
        else {
            panic!()
        };
        assert_eq!(state.state_name(), "connecting");
        assert!(state.apply_connected(generation, "/bin/bash"));
        assert_eq!(state.state_name(), "ready");
        assert_eq!(state.shell, "/bin/bash");
    }

    #[test]
    fn stale_connect_result_is_rejected_after_selection_change() {
        let mut state = ContainerTerminalControllerState::default();
        state.set_active(true);
        let TerminalSelectionAction::Connect { generation } = state.select_container("a", true)
        else {
            panic!()
        };
        assert!(matches!(
            state.select_container("b", true),
            TerminalSelectionAction::Connect { .. }
        ));
        assert!(!state.apply_connected(generation, "/bin/sh"));
        assert_eq!(state.container_id, "b");
        assert_eq!(state.state, ContainerTerminalState::Connecting);
    }

    #[test]
    fn leaving_tab_retains_ready_session_and_never_reconnects() {
        let mut state = ready();
        assert_eq!(state.set_active(false), TerminalSelectionAction::None);
        assert_eq!(state.state, ContainerTerminalState::Ready);
        assert_eq!(state.set_active(true), TerminalSelectionAction::None);
    }

    #[test]
    fn input_is_accepted_only_while_ready_and_invalidated_on_exit() {
        let mut state = ContainerTerminalControllerState::default();
        assert!(state.accept_input().is_none());
        state = ready();
        let input = state.accept_input().unwrap();
        let generation = state.generation;
        assert!(state.apply_exited(generation, Some(0)));
        assert!(state.input_generation > input);
        assert!(state.accept_input().is_none());
    }

    #[test]
    fn resize_is_debounced_to_latest_nonzero_geometry() {
        let mut state = ready();
        let first = state.request_resize(30, 100).unwrap();
        let second = state.request_resize(40, 120).unwrap();
        assert!(second > first);
        assert!(state.take_pending_resize(first).is_none());
        let pending = state.take_pending_resize(second).unwrap();
        assert_eq!((pending.rows, pending.columns), (40, 120));
        assert!(state.request_resize(0, 120).is_none());
    }

    #[test]
    fn selection_cleanup_closes_session_and_clears_shell() {
        let mut state = ready();
        assert!(matches!(
            state.clear_selection(),
            TerminalSelectionAction::Close { .. }
        ));
        assert!(state.container_id.is_empty());
        assert!(state.shell.is_empty());
        assert_eq!(state.state, ContainerTerminalState::Idle);
        assert!(state.pending_resize.is_none());
    }

    #[test]
    fn stopped_event_exits_matching_session_only() {
        let mut state = ready();
        assert_eq!(
            state.container_event_invalidated("other"),
            TerminalSelectionAction::None
        );
        assert_eq!(state.state, ContainerTerminalState::Ready);
        assert!(matches!(
            state.container_event_invalidated("container-a"),
            TerminalSelectionAction::Close { .. }
        ));
        assert_eq!(state.state, ContainerTerminalState::Exited);
        assert!(!state.selected_running);
    }

    #[test]
    fn shell_fallback_failure_has_exact_safe_message() {
        let mut state = ContainerTerminalControllerState::default();
        state.set_active(true);
        let TerminalSelectionAction::Connect { generation } = state.select_container("a", true)
        else {
            panic!()
        };
        assert!(state.apply_connect_error(generation, ContainerTerminalError::ShellNotFound));
        assert_eq!(state.state, ContainerTerminalState::Error);
        assert_eq!(state.error_kind, "shell_not_found");
        assert_eq!(
            state.error_message,
            "No supported shell was found in this container."
        );
    }

    #[test]
    fn stopped_and_paused_connect_failures_are_exited_not_generic_error() {
        for error in [
            ContainerTerminalError::NotRunning,
            ContainerTerminalError::Paused,
        ] {
            let mut state = ContainerTerminalControllerState::default();
            state.set_active(true);
            let TerminalSelectionAction::Connect { generation } = state.select_container("a", true)
            else {
                panic!()
            };
            assert!(state.apply_connect_error(generation, error));
            assert_eq!(state.state, ContainerTerminalState::Exited);
        }
    }

    #[test]
    fn stream_error_rejects_stale_generation_and_disables_resize() {
        let mut state = ready();
        let stale = state.generation.wrapping_sub(1);
        assert!(!state.apply_stream_error(stale, ContainerTerminalError::Disconnected));
        let generation = state.generation;
        assert!(state.apply_stream_error(generation, ContainerTerminalError::Disconnected));
        assert_eq!(state.state, ContainerTerminalState::Error);
        assert!(state.request_resize(20, 80).is_none());
    }

    #[test]
    fn vt100_interprets_cursor_movement_and_erase() {
        let mut renderer = ContainerTerminalRenderer::new(7, 4, 12);
        assert!(renderer.process(7, b"hello world"));
        assert!(renderer.process(7, b"\x1b[6D\x1b[KQt"));
        let screen = renderer.snapshot();
        assert_eq!(screen.rows[0], "helloQt");
        assert_eq!((screen.cursor_row, screen.cursor_column), (0, 7));
        assert!(!screen.rows.iter().any(|row| row.contains("\x1b")));
    }

    #[test]
    fn vt100_preserves_utf8_split_across_docker_chunks() {
        let mut renderer = ContainerTerminalRenderer::new(3, 2, 10);
        let bytes = "A界B".as_bytes();
        assert!(renderer.process(3, &bytes[..2]));
        assert!(renderer.process(3, &bytes[2..3]));
        assert!(renderer.process(3, &bytes[3..]));
        assert_eq!(renderer.snapshot().rows[0], "A界B");
    }

    #[test]
    fn vt100_switches_to_and_from_alternate_screen() {
        let mut renderer = ContainerTerminalRenderer::new(9, 3, 12);
        renderer.process(9, b"primary");
        renderer.process(9, b"\x1b[?1049halt");
        let alternate = renderer.snapshot();
        assert!(alternate.alternate_screen);
        assert_eq!(alternate.rows[0], "alt");
        renderer.process(9, b"\x1b[?1049l");
        let primary = renderer.snapshot();
        assert!(!primary.alternate_screen);
        assert_eq!(primary.rows[0], "primary");
    }

    #[test]
    fn resize_updates_emulator_geometry_and_preserves_content() {
        let mut renderer = ContainerTerminalRenderer::new(2, 2, 5);
        renderer.process(2, b"abc");
        assert!(renderer.resize(2, 4, 10));
        let screen = renderer.snapshot();
        assert_eq!((screen.row_count, screen.column_count), (4, 10));
        assert!(screen.rows.iter().any(|row| row.contains("abc")));
        assert!(!renderer.resize(2, 4, 10));
        assert!(!renderer.resize(2, 0, 10));
    }

    #[test]
    fn renderer_generation_rejects_stale_output_resize_and_scroll() {
        let mut renderer = ContainerTerminalRenderer::new(11, 2, 8);
        assert!(!renderer.process(10, b"secret stale output"));
        assert!(!renderer.resize(10, 30, 100));
        assert!(!renderer.scroll_lines(10, 5));
        assert_eq!(renderer.snapshot().rows, vec!["", ""]);
        renderer.reset(12, 3, 9);
        assert!(!renderer.process(11, b"old session"));
        assert!(renderer.process(12, b"new"));
        assert_eq!(renderer.snapshot().rows[0], "new");
    }

    #[test]
    fn key_mapping_covers_navigation_controls_and_application_mode() {
        assert_eq!(terminal_key_bytes("Enter", false), Some(b"\r".to_vec()));
        assert_eq!(terminal_key_bytes("Backspace", false), Some(vec![0x7f]));
        assert_eq!(terminal_key_bytes("Tab", false), Some(b"\t".to_vec()));
        assert_eq!(terminal_key_bytes("Escape", false), Some(vec![0x1b]));
        assert_eq!(terminal_key_bytes("Up", false), Some(b"\x1b[A".to_vec()));
        assert_eq!(terminal_key_bytes("Up", true), Some(b"\x1bOA".to_vec()));
        assert_eq!(terminal_key_bytes("Home", false), Some(b"\x1b[H".to_vec()));
        assert_eq!(terminal_key_bytes("End", true), Some(b"\x1bOF".to_vec()));
        assert_eq!(
            terminal_key_bytes("PageUp", false),
            Some(b"\x1b[5~".to_vec())
        );
        assert_eq!(
            terminal_key_bytes("PageDown", false),
            Some(b"\x1b[6~".to_vec())
        );
        assert_eq!(
            terminal_key_bytes("Delete", false),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            terminal_key_bytes("Insert", false),
            Some(b"\x1b[2~".to_vec())
        );
        assert_eq!(terminal_key_bytes("Ctrl+A", false), Some(vec![1]));
        assert_eq!(terminal_key_bytes("Ctrl+Z", false), Some(vec![26]));
        assert_eq!(terminal_key_bytes("F1", false), None);
    }

    #[test]
    fn bracketed_paste_is_wrapped_only_when_requested() {
        assert_eq!(terminal_paste_bytes("hello\n", false), b"hello\n");
        assert_eq!(
            terminal_paste_bytes("hello\n", true),
            b"\x1b[200~hello\n\x1b[201~"
        );
    }
}
