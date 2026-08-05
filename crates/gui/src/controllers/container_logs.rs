//! Pure state and bounded viewport for Docker container logs.
//!
//! Incoming stream items are applied in batches by `container_live_bridge`;
//! no task or QML object is created per log line.

use std::collections::VecDeque;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use tuxstack_domain::{ContainerLogsOptions, LogLine, LogStream};

pub const DEFAULT_LOG_TAIL: usize = 1000;
pub const LOG_TAIL_VALUES: &[usize] = &[100, 500, 1000, 5000];
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTail {
    Lines(usize),
    All,
}

impl Default for LogTail {
    fn default() -> Self {
        Self::Lines(DEFAULT_LOG_TAIL)
    }
}

impl LogTail {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        let Ok(lines) = value.parse::<usize>() else {
            return Err("Tail must be 100, 500, 1000, 5000, or all.");
        };
        if LOG_TAIL_VALUES.contains(&lines) {
            Ok(Self::Lines(lines))
        } else {
            Err("Tail must be 100, 500, 1000, 5000, or all.")
        }
    }

    pub fn as_str(self) -> String {
        match self {
            Self::Lines(lines) => lines.to_string(),
            Self::All => "all".into(),
        }
    }

    pub fn docker_value(self) -> Option<usize> {
        match self {
            Self::Lines(lines) => Some(lines),
            Self::All => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LogSince {
    #[default]
    All,
    Timestamp(DateTime<Utc>),
}

impl LogSince {
    pub fn docker_value(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::All => None,
            Self::Timestamp(value) => Some(*value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogMemberOption {
    pub id: String,
    pub name: String,
    pub label: String,
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
    pub validation_error: String,
    pub stdout: bool,
    pub stderr: bool,
    pub tail: LogTail,
    pub since: LogSince,
    pub since_input: String,
    pub timestamps: bool,
    pub follow: bool,
    pub paused: bool,
    pub wrap: bool,
    pub search_query: String,
    pub member_filter_id: String,
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
            validation_error: String::new(),
            stdout: true,
            stderr: true,
            tail: LogTail::default(),
            since: LogSince::default(),
            since_input: String::new(),
            timestamps: true,
            follow: true,
            paused: false,
            wrap: false,
            search_query: String::new(),
            member_filter_id: String::new(),
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
        self.member_filter_id.clear();
        self.clear_viewport();
        self.paused = false;
        self.error_message.clear();
        self.validation_error.clear();
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
            return self.clear_validation_error();
        }
        self.follow = follow;
        self.advance_stream_generation();
        true
    }

    pub fn set_timestamps(&mut self, timestamps: bool) -> bool {
        if self.timestamps == timestamps {
            return self.clear_validation_error();
        }
        self.timestamps = timestamps;
        self.advance_stream_generation();
        true
    }

    pub fn set_stdout(&mut self, enabled: bool) -> bool {
        if !enabled && !self.stderr {
            return self.set_validation_error("At least one of stdout or stderr must be enabled.");
        }
        if self.stdout == enabled {
            return self.clear_validation_error();
        }
        self.stdout = enabled;
        self.advance_stream_generation();
        true
    }

    pub fn set_stderr(&mut self, enabled: bool) -> bool {
        if !enabled && !self.stdout {
            return self.set_validation_error("At least one of stdout or stderr must be enabled.");
        }
        if self.stderr == enabled {
            return self.clear_validation_error();
        }
        self.stderr = enabled;
        self.advance_stream_generation();
        true
    }

    pub fn set_tail(&mut self, value: &str) -> bool {
        let tail = match LogTail::parse(value) {
            Ok(tail) => tail,
            Err(message) => return self.set_validation_error(message),
        };
        if self.tail == tail {
            return self.clear_validation_error();
        }
        self.tail = tail;
        self.advance_stream_generation();
        true
    }

    pub fn set_since(&mut self, value: &str) -> bool {
        self.set_since_at(value, Utc::now())
    }

    fn set_since_at(&mut self, value: &str, now: DateTime<Utc>) -> bool {
        let input = value.trim();
        if input.eq_ignore_ascii_case(&self.since_input) {
            return self.clear_validation_error();
        }
        let since = match parse_since_at(input, now) {
            Ok(since) => since,
            Err(message) => return self.set_validation_error(&message),
        };
        self.since = since;
        self.since_input =
            if input.eq_ignore_ascii_case("all") || input.eq_ignore_ascii_case("all time") {
                String::new()
            } else {
                input.to_string()
            };
        self.advance_stream_generation();
        true
    }

    pub fn set_member_filter(&mut self, container_id: &str) -> bool {
        let container_id = container_id.trim();
        if !container_id.is_empty() && !self.targets.iter().any(|target| target.id == container_id)
        {
            return self.set_validation_error("The selected log group member is unavailable.");
        }
        if self.member_filter_id == container_id {
            return self.clear_validation_error();
        }
        self.member_filter_id = container_id.to_string();
        self.clear_validation_error();
        true
    }

    pub fn member_options(&self) -> Vec<LogMemberOption> {
        let mut result = Vec::with_capacity(self.targets.len() + 1);
        result.push(LogMemberOption {
            id: String::new(),
            name: "All containers".into(),
            label: "All containers".into(),
        });
        result.extend(self.targets.iter().map(|target| {
            let short_id = target.id.chars().take(12).collect::<String>();
            LogMemberOption {
                id: target.id.clone(),
                name: target.name.clone(),
                label: format!("{} ({short_id})", target.name),
            }
        }));
        result
    }

    pub fn docker_options(&self, running: bool, include_history: bool) -> ContainerLogsOptions {
        ContainerLogsOptions {
            stdout: self.stdout,
            stderr: self.stderr,
            timestamps: self.timestamps,
            follow: self.follow && running,
            tail: if include_history {
                self.tail.docker_value()
            } else {
                Some(0)
            },
            since: self.since.docker_value(),
            until: None,
        }
    }

    pub fn visible_entries(&self) -> Vec<&LogViewportLine> {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                (self.member_filter_id.is_empty() || entry.container_id == self.member_filter_id)
                    && match entry.stream.as_str() {
                        "stdout" => self.stdout,
                        "stderr" => self.stderr,
                        _ => true,
                    }
                    && (query.is_empty()
                        || entry.display.to_ascii_lowercase().contains(&query)
                        || entry.stream.to_ascii_lowercase().contains(&query))
            })
            .collect()
    }

    fn advance_stream_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.error_message.clear();
        self.validation_error.clear();
        self.status = if self.should_stream() {
            LiveLogsStatus::Streaming
        } else {
            LiveLogsStatus::Idle
        };
    }

    fn set_validation_error(&mut self, message: &str) -> bool {
        if self.validation_error == message {
            return false;
        }
        self.validation_error = message.to_string();
        true
    }

    fn clear_validation_error(&mut self) -> bool {
        if self.validation_error.is_empty() {
            return false;
        }
        self.validation_error.clear();
        true
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
        self.validation_error.clear();
    }
}

fn parse_since_at(input: &str, now: DateTime<Utc>) -> Result<LogSince, String> {
    if input.is_empty()
        || input.eq_ignore_ascii_case("all")
        || input.eq_ignore_ascii_case("all time")
    {
        return Ok(LogSince::All);
    }
    if let Ok(seconds) = input.parse::<i64>() {
        return validated_since_timestamp(seconds);
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(input) {
        return validated_since_timestamp(value.timestamp());
    }

    let lower = input.to_ascii_lowercase();
    let relative = lower.strip_suffix(" ago").unwrap_or(&lower);
    let split = relative
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(relative.len());
    let (amount, unit) = relative.split_at(split);
    let seconds_per_unit = match unit.trim() {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(1_i64),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(60),
        "h" | "hour" | "hours" => Some(60 * 60),
        "d" | "day" | "days" => Some(24 * 60 * 60),
        _ => None,
    };
    if let (Ok(amount), Some(seconds_per_unit)) = (amount.parse::<i64>(), seconds_per_unit)
        && amount > 0
        && let Some(seconds) = amount.checked_mul(seconds_per_unit)
        && let Some(value) = now.checked_sub_signed(TimeDelta::seconds(seconds))
    {
        return validated_since_timestamp(value.timestamp());
    }
    Err(
        "Since must be all, a Unix timestamp, RFC 3339, or a duration such as 5m, 1h, or 24h."
            .into(),
    )
}

fn validated_since_timestamp(seconds: i64) -> Result<LogSince, String> {
    // docker-core's locked Bollard mapping currently serializes this domain
    // through i32. Reject instead of silently wrapping an otherwise valid
    // chrono/Bollard i64 timestamp.
    i32::try_from(seconds).map_err(|_| {
        "Since is outside the Docker API timestamp range supported here.".to_string()
    })?;
    DateTime::from_timestamp(seconds, 0)
        .map(LogSince::Timestamp)
        .ok_or_else(|| "Since is outside the supported timestamp range.".to_string())
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

    fn line(message: &str) -> LogLine {
        stream_line(LogStream::Stdout, message)
    }

    fn stream_line(stream: LogStream, message: &str) -> LogLine {
        LogLine {
            timestamp: Some(Utc.timestamp_opt(10, 0).unwrap()),
            stream,
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
        assert_eq!(state.tail, LogTail::Lines(1000));
        assert!(state.stdout);
        assert!(state.stderr);
        assert!(state.follow);
        assert!(state.timestamps);
        assert_eq!(state.since, LogSince::All);
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
    fn stream_filters_require_one_enabled_and_clear_validation_on_recovery() {
        let (mut state, _) = active_state();
        assert!(state.set_stdout(false));
        let generation = state.generation;
        assert!(state.set_stderr(false));
        assert!(state.stderr);
        assert_eq!(state.generation, generation);
        assert_eq!(
            state.validation_error,
            "At least one of stdout or stderr must be enabled."
        );
        assert!(state.set_stdout(true));
        assert!(state.validation_error.is_empty());
        assert!(state.generation > generation);
    }

    #[test]
    fn stdout_and_stderr_filters_are_non_destructive_and_map_to_docker() {
        let (mut state, generation) = active_state();
        state.apply_batch(
            generation,
            vec![
                ("a".into(), stream_line(LogStream::Stdout, "out")),
                ("a".into(), stream_line(LogStream::Stderr, "err")),
            ],
        );
        let old_generation = state.generation;
        assert!(state.set_stdout(false));
        assert!(state.generation > old_generation);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.visible_entries()[0].message, "err");
        let options = state.docker_options(true, true);
        assert!(!options.stdout);
        assert!(options.stderr);
        assert!(options.follow);
    }

    #[test]
    fn tail_validation_and_all_map_to_docker_options() {
        let (mut state, _) = active_state();
        let generation = state.generation;
        assert!(state.set_tail("5000"));
        assert_eq!(state.tail, LogTail::Lines(5000));
        assert_eq!(state.docker_options(true, true).tail, Some(5000));
        assert!(state.generation > generation);

        let generation = state.generation;
        assert!(state.set_tail("all"));
        assert_eq!(state.docker_options(true, true).tail, None);
        assert!(state.generation > generation);

        let generation = state.generation;
        assert!(state.set_tail("42"));
        assert_eq!(state.generation, generation);
        assert!(!state.validation_error.is_empty());
        assert_eq!(state.tail, LogTail::All);
    }

    #[test]
    fn since_accepts_unix_rfc3339_and_relative_values_and_maps_exactly() {
        let (mut state, _) = active_state();
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();

        assert!(state.set_since_at("1699996400", now));
        assert_eq!(
            state.docker_options(true, true).since,
            Some(Utc.timestamp_opt(1_699_996_400, 0).unwrap())
        );
        assert!(state.set_since_at("2023-11-14T20:13:20Z", now));
        assert_eq!(
            state.docker_options(true, true).since,
            Some(Utc.timestamp_opt(1_699_992_800, 0).unwrap())
        );
        assert!(state.set_since_at("30m", now));
        assert_eq!(
            state.docker_options(true, true).since,
            Some(Utc.timestamp_opt(1_699_998_200, 0).unwrap())
        );
        assert!(state.set_since_at("all", now));
        assert_eq!(state.docker_options(true, true).since, None);
    }

    #[test]
    fn invalid_since_preserves_stream_generation_and_previous_option() {
        let (mut state, _) = active_state();
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        state.set_since_at("1h", now);
        let generation = state.generation;
        let since = state.since.clone();
        assert!(state.set_since_at("yesterday-ish", now));
        assert_eq!(state.generation, generation);
        assert_eq!(state.since, since);
        assert!(!state.validation_error.is_empty());
        assert!(state.set_since_at("2200000000", now));
        assert_eq!(state.generation, generation);
        assert_eq!(state.since, since);
    }

    #[test]
    fn group_member_filter_republishes_without_restart_or_data_loss() {
        let mut state = ContainerLogsState::default();
        state.set_selection(
            "group",
            "g",
            &["aaaaaaaaaaaa111".into(), "bbbbbbbbbbbb222".into()],
            &["running".into(), "running".into()],
            &["api".into(), "db".into()],
        );
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        state.apply_batch(
            generation,
            vec![
                ("aaaaaaaaaaaa111".into(), line("api line")),
                ("bbbbbbbbbbbb222".into(), line("db line")),
            ],
        );
        let stream_generation = state.generation;
        assert!(state.set_member_filter("bbbbbbbbbbbb222"));
        assert_eq!(state.generation, stream_generation);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.visible_entries()[0].message, "db line");
        assert!(state.save_text().contains("db line"));
        assert!(!state.save_text().contains("api line"));

        let options = state.member_options();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].id, "");
        assert_eq!(options[0].label, "All containers");
        assert_eq!(options[1].name, "api");
        assert!(options[1].label.contains("aaaaaaaaaaaa"));
    }

    #[test]
    fn stream_affecting_options_advance_generation_but_view_filters_do_not() {
        let (mut state, _) = active_state();
        let mut generation = state.generation;
        assert!(state.set_timestamps(false));
        assert!(state.generation > generation);
        generation = state.generation;
        assert!(state.set_follow(false));
        assert!(state.generation > generation);
        generation = state.generation;
        assert!(state.set_tail("100"));
        assert!(state.generation > generation);
        generation = state.generation;
        assert!(state.set_since_at("1h", Utc.timestamp_opt(1_700_000_000, 0).unwrap()));
        assert!(state.generation > generation);
        generation = state.generation;
        assert!(state.set_search("needle"));
        assert_eq!(state.generation, generation);
        assert!(state.set_member_filter("a"));
        assert_eq!(state.generation, generation);
    }

    #[test]
    fn stopped_target_never_follows_but_keeps_selected_history_options() {
        let state = ContainerLogsState::default();
        let options = state.docker_options(false, true);
        assert!(!options.follow);
        assert_eq!(options.tail, Some(DEFAULT_LOG_TAIL));
        assert_eq!(state.docker_options(true, false).tail, Some(0));
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
