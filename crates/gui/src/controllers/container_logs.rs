//! Pure state and bounded viewport for Docker container logs.
//!
//! Incoming stream items are applied in batches by `container_live_bridge`;
//! no task or QML object is created per log line.

use std::collections::VecDeque;

use chrono::{DateTime, SecondsFormat, Utc};
use tuxstack_docker_core::{LogLine, LogStream};

pub const DEFAULT_LOG_TAIL: usize = 1000;
pub const MAX_LOG_LINES: usize = 20_000;
pub const MAX_LOG_BYTES: usize = 32 * 1024 * 1024;
pub const DISCARDED_NOTICE: &str = "Older log entries were discarded from this view.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LiveLogsStatus {
    #[default]
    Idle,
    Streaming,
    Ready,
    Error,
}

impl LiveLogsStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Streaming => "streaming",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogTarget {
    pub id: String,
    pub name: String,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogViewportLine {
    pub sequence: u64,
    pub container_id: String,
    pub container_name: String,
    pub stream: String,
    pub timestamp: String,
    pub message: String,
    pub display: String,
    pub byte_size: usize,
}

impl LogViewportLine {
    pub fn from_domain(
        sequence: u64,
        target: &LogTarget,
        group: bool,
        timestamps: bool,
        line: LogLine,
    ) -> Vec<Self> {
        let stream = stream_name(line.stream).to_string();
        let timestamp = if timestamps {
            line.timestamp.map(format_timestamp).unwrap_or_default()
        } else {
            String::new()
        };
        let mut messages = line
            .message
            .split_terminator('\n')
            .map(|value| value.strip_suffix('\r').unwrap_or(value).to_string())
            .collect::<Vec<_>>();
        if messages.is_empty() {
            messages.push(String::new());
        }
        messages
            .into_iter()
            .enumerate()
            .map(|(offset, message)| {
                let mut display = String::new();
                if group {
                    display.push('[');
                    display.push_str(&target.name);
                    display.push_str("] ");
                }
                if !timestamp.is_empty() {
                    display.push_str(&timestamp);
                    display.push(' ');
                }
                display.push_str(&message);
                let byte_size = display.len()
                    + target.id.len()
                    + target.name.len()
                    + stream.len()
                    + timestamp.len();
                Self {
                    sequence: sequence.saturating_add(offset as u64),
                    container_id: target.id.clone(),
                    container_name: target.name.clone(),
                    stream: stream.clone(),
                    timestamp: timestamp.clone(),
                    message,
                    display,
                    byte_size,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ContainerLogsState {
    pub selection_kind: String,
    pub selection_id: String,
    pub targets: Vec<LogTarget>,
    pub active: bool,
    pub generation: u64,
    pub status: LiveLogsStatus,
    pub error_message: String,
    pub tail: usize,
    pub timestamps: bool,
    pub follow: bool,
    pub paused: bool,
    pub wrap: bool,
    pub search_query: String,
    pub entries: VecDeque<LogViewportLine>,
    pub pending: VecDeque<LogViewportLine>,
    pub viewport_bytes: usize,
    pub pending_bytes: usize,
    pub discarded: bool,
    pub next_sequence: u64,
}

impl Default for ContainerLogsState {
    fn default() -> Self {
        Self {
            selection_kind: "none".into(),
            selection_id: String::new(),
            targets: Vec::new(),
            active: false,
            generation: 0,
            status: LiveLogsStatus::Idle,
            error_message: String::new(),
            tail: DEFAULT_LOG_TAIL,
            timestamps: true,
            follow: true,
            paused: false,
            wrap: false,
            search_query: String::new(),
            entries: VecDeque::new(),
            pending: VecDeque::new(),
            viewport_bytes: 0,
            pending_bytes: 0,
            discarded: false,
            next_sequence: 1,
        }
    }
}

impl ContainerLogsState {
    pub fn set_selection(
        &mut self,
        kind: &str,
        id: &str,
        ids: &[String],
        states: &[String],
        names: &[String],
    ) -> bool {
        let kind = normalize_kind(kind);
        let targets = build_targets(kind, id, ids, states, names);
        if self.selection_kind == kind && self.selection_id == id && self.targets == targets {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.selection_kind = kind.into();
        self.selection_id = if kind == "none" {
            String::new()
        } else {
            id.into()
        };
        self.targets = targets;
        self.clear_viewport();
        self.paused = false;
        self.error_message.clear();
        self.status = if self.should_stream() {
            LiveLogsStatus::Streaming
        } else {
            LiveLogsStatus::Idle
        };
        true
    }

    pub fn set_active(&mut self, active: bool) -> bool {
        if self.active == active {
            return false;
        }
        self.active = active;
        self.generation = self.generation.wrapping_add(1);
        self.status = if self.should_stream() {
            LiveLogsStatus::Streaming
        } else {
            LiveLogsStatus::Idle
        };
        true
    }

    pub fn should_stream(&self) -> bool {
        self.active && self.selection_kind != "none" && !self.targets.is_empty()
    }

    pub fn begin_stream(&mut self) -> Option<(u64, Vec<LogTarget>)> {
        if !self.should_stream() {
            self.status = LiveLogsStatus::Idle;
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.error_message.clear();
        self.status = LiveLogsStatus::Streaming;
        Some((self.generation, self.targets.clone()))
    }

    /// Apply one bridge-delivered batch in arrival order. The bridge batches
    /// many stream items before crossing to the Qt thread.
    pub fn apply_batch(&mut self, generation: u64, batch: Vec<(String, LogLine)>) -> bool {
        if generation != self.generation || !self.should_stream() || batch.is_empty() {
            return false;
        }
        let group = self.selection_kind == "group";
        let mut changed = false;
        for (container_id, line) in batch {
            let Some(target) = self
                .targets
                .iter()
                .find(|target| target.id == container_id)
                .cloned()
            else {
                continue;
            };
            let lines = LogViewportLine::from_domain(
                self.next_sequence,
                &target,
                group,
                self.timestamps,
                line,
            );
            self.next_sequence = self.next_sequence.saturating_add(lines.len() as u64);
            for line in lines {
                if self.paused {
                    self.pending_bytes = self.pending_bytes.saturating_add(line.byte_size);
                    self.pending.push_back(line);
                } else {
                    self.viewport_bytes = self.viewport_bytes.saturating_add(line.byte_size);
                    self.entries.push_back(line);
                }
                enforce_combined_cap(
                    &mut self.entries,
                    &mut self.viewport_bytes,
                    &mut self.pending,
                    &mut self.pending_bytes,
                    &mut self.discarded,
                );
                changed = true;
            }
        }
        if changed {
            self.status = LiveLogsStatus::Ready;
            self.error_message.clear();
        }
        changed
    }

    pub fn apply_error(&mut self, generation: u64, message: &str) -> bool {
        if generation != self.generation || !self.active {
            return false;
        }
        self.status = LiveLogsStatus::Error;
        self.error_message = message.to_string();
        true
    }

    pub fn set_paused(&mut self, paused: bool) -> bool {
        if self.paused == paused {
            return false;
        }
        self.paused = paused;
        if !paused {
            while let Some(line) = self.pending.pop_front() {
                self.pending_bytes = self.pending_bytes.saturating_sub(line.byte_size);
                self.viewport_bytes = self.viewport_bytes.saturating_add(line.byte_size);
                self.entries.push_back(line);
                enforce_cap(
                    &mut self.entries,
                    &mut self.viewport_bytes,
                    &mut self.discarded,
                );
            }
        }
        true
    }

    pub fn set_search(&mut self, query: &str) -> bool {
        let query = query.to_string();
        if self.search_query == query {
            return false;
        }
        self.search_query = query;
        true
    }

    pub fn set_wrap(&mut self, wrap: bool) -> bool {
        if self.wrap == wrap {
            return false;
        }
        self.wrap = wrap;
        true
    }

    pub fn set_follow(&mut self, follow: bool) -> bool {
        if self.follow == follow {
            return false;
        }
        self.follow = follow;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    pub fn set_timestamps(&mut self, timestamps: bool) -> bool {
        if self.timestamps == timestamps {
            return false;
        }
        self.timestamps = timestamps;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    pub fn visible_entries(&self) -> Vec<&LogViewportLine> {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.display.to_ascii_lowercase().contains(&query)
                    || entry.stream.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    /// Clear means the bounded UI viewport only; Docker history is untouched.
    pub fn clear_viewport(&mut self) {
        self.entries.clear();
        self.pending.clear();
        self.viewport_bytes = 0;
        self.pending_bytes = 0;
        self.discarded = false;
    }

    pub fn save_text(&self) -> String {
        let visible = self.visible_entries();
        let mut result =
            String::with_capacity(visible.iter().map(|entry| entry.display.len() + 1).sum());
        if self.discarded {
            result.push_str(DISCARDED_NOTICE);
            result.push('\n');
        }
        for entry in visible {
            result.push_str(&entry.display);
            result.push('\n');
        }
        result
    }

    pub fn shutdown(&mut self) {
        self.active = false;
        self.generation = self.generation.wrapping_add(1);
        self.status = LiveLogsStatus::Idle;
        self.clear_viewport();
        self.error_message.clear();
    }
}

fn normalize_kind(kind: &str) -> &'static str {
    match kind.trim().to_ascii_lowercase().as_str() {
        "container" => "container",
        "group" => "group",
        _ => "none",
    }
}

fn build_targets(
    kind: &str,
    selection_id: &str,
    ids: &[String],
    states: &[String],
    names: &[String],
) -> Vec<LogTarget> {
    if kind == "none" || selection_id.trim().is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        if id.trim().is_empty() {
            continue;
        }
        if result.iter().any(|target: &LogTarget| target.id == *id) {
            continue;
        }
        result.push(LogTarget {
            id: id.clone(),
            name: names
                .get(index)
                .filter(|name| !name.is_empty())
                .cloned()
                .unwrap_or_else(|| id.chars().take(12).collect()),
            running: states
                .get(index)
                .is_some_and(|state| state.eq_ignore_ascii_case("running")),
        });
        if kind == "container" {
            break;
        }
    }
    result
}

fn enforce_cap(entries: &mut VecDeque<LogViewportLine>, bytes: &mut usize, discarded: &mut bool) {
    while entries.len() > MAX_LOG_LINES || *bytes > MAX_LOG_BYTES {
        let Some(removed) = entries.pop_front() else {
            break;
        };
        *bytes = bytes.saturating_sub(removed.byte_size);
        *discarded = true;
    }
}

fn enforce_combined_cap(
    entries: &mut VecDeque<LogViewportLine>,
    entry_bytes: &mut usize,
    pending: &mut VecDeque<LogViewportLine>,
    pending_bytes: &mut usize,
    discarded: &mut bool,
) {
    while entries.len().saturating_add(pending.len()) > MAX_LOG_LINES
        || entry_bytes.saturating_add(*pending_bytes) > MAX_LOG_BYTES
    {
        if let Some(removed) = entries.pop_front() {
            *entry_bytes = entry_bytes.saturating_sub(removed.byte_size);
        } else if let Some(removed) = pending.pop_front() {
            *pending_bytes = pending_bytes.saturating_sub(removed.byte_size);
        } else {
            break;
        }
        *discarded = true;
    }
}

fn stream_name(stream: LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::Console => "console",
        LogStream::Unknown => "unknown",
    }
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn target(id: &str, running: bool) -> LogTarget {
        LogTarget {
            id: id.into(),
            name: format!("name-{id}"),
            running,
        }
    }

    fn line(message: &str) -> LogLine {
        LogLine {
            timestamp: Some(Utc.timestamp_opt(10, 0).unwrap()),
            stream: LogStream::Stdout,
            message: message.into(),
        }
    }

    fn active_state() -> (ContainerLogsState, u64) {
        let mut state = ContainerLogsState::default();
        state.set_selection(
            "container",
            "a",
            &["a".into()],
            &["running".into()],
            &["alpha".into()],
        );
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        (state, generation)
    }

    #[test]
    fn defaults_match_logs_contract() {
        let state = ContainerLogsState::default();
        assert_eq!(state.tail, 1000);
        assert!(state.follow);
        assert!(state.timestamps);
        assert!(!state.wrap);
    }

    #[test]
    fn blank_selection_does_not_stream() {
        let mut state = ContainerLogsState::default();
        state.set_active(true);
        assert!(state.begin_stream().is_none());
    }

    #[test]
    fn group_retains_every_member_stream() {
        let mut state = ContainerLogsState::default();
        let ids = (0..12).map(|n| format!("id{n}")).collect::<Vec<_>>();
        let states = (0..12).map(|_| "running".into()).collect::<Vec<_>>();
        let names = ids.clone();
        state.set_selection("group", "g", &ids, &states, &names);
        assert_eq!(state.targets.len(), 12);
    }

    #[test]
    fn stopped_targets_are_retained_for_historical_logs() {
        let mut state = ContainerLogsState::default();
        state.set_selection(
            "group",
            "g",
            &["a".into(), "b".into()],
            &["running".into(), "exited".into()],
            &["A".into(), "B".into()],
        );
        assert_eq!(
            state.targets,
            vec![
                LogTarget {
                    id: "a".into(),
                    name: "A".into(),
                    running: true,
                },
                LogTarget {
                    id: "b".into(),
                    name: "B".into(),
                    running: false,
                },
            ]
        );
    }

    #[test]
    fn group_lines_are_prefixed_in_arrival_order() {
        let mut state = ContainerLogsState::default();
        state.set_selection(
            "group",
            "g",
            &["a".into(), "b".into()],
            &["running".into(), "running".into()],
            &["A".into(), "B".into()],
        );
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        assert!(state.apply_batch(
            generation,
            vec![("b".into(), line("second")), ("a".into(), line("first"))]
        ));
        assert!(state.entries[0].display.starts_with("[B] "));
        assert!(state.entries[1].display.starts_with("[A] "));
        assert!(state.entries[0].sequence < state.entries[1].sequence);
    }

    #[test]
    fn multiline_chunks_are_split_into_viewport_lines() {
        let (mut state, generation) = active_state();
        state.apply_batch(generation, vec![("a".into(), line("one\ntwo\n"))]);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.entries[0].message, "one");
        assert_eq!(state.entries[1].message, "two");
    }

    #[test]
    fn stale_generation_and_unknown_target_are_rejected() {
        let (mut state, generation) = active_state();
        assert!(!state.apply_batch(generation - 1, vec![("a".into(), line("old"))]));
        assert!(!state.apply_batch(generation, vec![("x".into(), line("unknown"))]));
        assert!(state.entries.is_empty());
    }

    #[test]
    fn leaving_tab_invalidates_stream_immediately() {
        let (mut state, generation) = active_state();
        state.set_active(false);
        assert!(!state.apply_batch(generation, vec![("a".into(), line("late"))]));
        assert_eq!(state.status, LiveLogsStatus::Idle);
    }

    #[test]
    fn pause_queues_and_resume_flushes_in_order() {
        let (mut state, generation) = active_state();
        state.set_paused(true);
        state.apply_batch(
            generation,
            vec![("a".into(), line("one")), ("a".into(), line("two"))],
        );
        assert!(state.entries.is_empty());
        assert_eq!(state.pending.len(), 2);
        state.set_paused(false);
        assert_eq!(state.entries[0].message, "one");
        assert_eq!(state.entries[1].message, "two");
        assert!(state.pending.is_empty());
    }

    #[test]
    fn search_is_case_insensitive_and_non_destructive() {
        let (mut state, generation) = active_state();
        state.apply_batch(
            generation,
            vec![("a".into(), line("Error HERE")), ("a".into(), line("ok"))],
        );
        state.set_search("error");
        assert_eq!(state.visible_entries().len(), 1);
        assert_eq!(state.entries.len(), 2);
    }

    #[test]
    fn clear_only_affects_current_viewport() {
        let (mut state, generation) = active_state();
        state.apply_batch(generation, vec![("a".into(), line("old"))]);
        state.clear_viewport();
        assert!(state.entries.is_empty());
        assert!(state.should_stream());
        assert!(state.apply_batch(generation, vec![("a".into(), line("new"))]));
        assert_eq!(state.entries[0].message, "new");
    }

    #[test]
    fn line_cap_discards_oldest_and_sets_notice() {
        let (mut state, generation) = active_state();
        for n in 0..=MAX_LOG_LINES {
            state.apply_batch(generation, vec![("a".into(), line(&n.to_string()))]);
        }
        assert_eq!(state.entries.len(), MAX_LOG_LINES);
        assert_eq!(state.entries.front().unwrap().message, "1");
        assert!(state.discarded);
        assert!(state.save_text().starts_with(DISCARDED_NOTICE));
    }

    #[test]
    fn byte_cap_handles_single_huge_line_without_overflow() {
        let (mut state, generation) = active_state();
        state.apply_batch(
            generation,
            vec![("a".into(), line(&"x".repeat(MAX_LOG_BYTES + 1)))],
        );
        assert!(state.entries.is_empty());
        assert_eq!(state.viewport_bytes, 0);
        assert!(state.discarded);
    }

    #[test]
    fn save_uses_current_filtered_bounded_viewport() {
        let (mut state, generation) = active_state();
        state.apply_batch(
            generation,
            vec![("a".into(), line("keep")), ("a".into(), line("omit"))],
        );
        state.set_search("keep");
        let text = state.save_text();
        assert!(text.contains("keep"));
        assert!(!text.contains("omit"));
    }

    #[test]
    fn timestamp_toggle_advances_generation_for_real_restart() {
        let (mut state, _) = active_state();
        let old = state.generation;
        assert!(state.set_timestamps(false));
        assert!(state.generation > old);
    }

    #[test]
    fn shutdown_cancels_and_clears_memory() {
        let (mut state, generation) = active_state();
        state.apply_batch(generation, vec![("a".into(), line("data"))]);
        let old = state.generation;
        state.shutdown();
        assert!(state.generation > old);
        assert!(!state.active);
        assert!(state.entries.is_empty());
    }
}
