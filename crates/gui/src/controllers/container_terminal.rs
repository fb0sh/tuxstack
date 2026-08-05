//! Pure lifecycle controller for a real Docker exec terminal.
//!
//! This state machine never interprets terminal bytes. A terminal emulator
//! surface must consume the backend stream; exposing raw/ANSI bytes through a
//! QML TextArea is intentionally outside this controller's API.

use tuxstack_docker_core::{ContainerTerminalError, ContainerTerminalState};

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
            rows: 24,
            columns: 80,
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
}
