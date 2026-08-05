//! Pure state for live container statistics.
//!
//! Docker I/O and Qt notifications belong to `container_live_bridge`; this
//! module only validates selections, rejects stale samples, aggregates group
//! samples, and keeps the bounded in-memory history.

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use tuxstack_domain::ContainerStats;

pub const STATS_HISTORY_CAPACITY: usize = 600;
pub const MAX_CONCURRENT_STATS_REQUESTS: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LiveStatsStatus {
    #[default]
    Idle,
    Streaming,
    Ready,
    Error,
}

impl LiveStatsStatus {
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
pub struct StatsTarget {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsReading {
    pub cpu_percent: f64,
    /// Docker's raw `memory_stats.usage` value.
    pub memory_raw_bytes: u64,
    /// Working set is optional because the currently exposed service sample
    /// does not contain cache/inactive-file counters on every daemon.
    pub memory_working_set_bytes: Option<u64>,
    pub memory_limit_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    pub pids: u64,
    pub sampled_at: DateTime<Utc>,
}

impl From<ContainerStats> for StatsReading {
    fn from(value: ContainerStats) -> Self {
        Self {
            cpu_percent: finite_non_negative(value.cpu_percent),
            memory_raw_bytes: value.memory_usage_bytes,
            memory_working_set_bytes: None,
            memory_limit_bytes: value.memory_limit_bytes,
            network_rx_bytes: value.network_rx_bytes,
            network_tx_bytes: value.network_tx_bytes,
            block_read_bytes: value.block_read_bytes,
            block_write_bytes: value.block_write_bytes,
            pids: value.pids.unwrap_or(0),
            sampled_at: value.sampled_at,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AggregateStats {
    pub cpu_percent: f64,
    pub memory_raw_bytes: u64,
    pub memory_working_set_bytes: Option<u64>,
    pub memory_limit_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    pub pids: u64,
    pub sampled_at: Option<DateTime<Utc>>,
    pub reporting_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsHistoryPoint {
    pub sampled_at: DateTime<Utc>,
    pub cpu_percent: f64,
    pub memory_raw_bytes: u64,
    pub memory_working_set_bytes: Option<u64>,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ContainerStatsState {
    pub selection_kind: String,
    pub selection_id: String,
    pub targets: Vec<StatsTarget>,
    pub active: bool,
    pub generation: u64,
    pub status: LiveStatsStatus,
    pub error_message: String,
    pub latest: HashMap<String, StatsReading>,
    pub history: VecDeque<StatsHistoryPoint>,
}

impl Default for ContainerStatsState {
    fn default() -> Self {
        Self {
            selection_kind: "none".into(),
            selection_id: String::new(),
            targets: Vec::new(),
            active: false,
            generation: 0,
            status: LiveStatsStatus::Idle,
            error_message: String::new(),
            latest: HashMap::new(),
            history: VecDeque::with_capacity(STATS_HISTORY_CAPACITY),
        }
    }
}

impl ContainerStatsState {
    /// Set one authoritative selection. For groups, every running member is
    /// retained; the bridge limits concurrent Docker requests.
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
        self.reset_samples();
        true
    }

    pub fn set_active(&mut self, active: bool) -> bool {
        if self.active == active {
            return false;
        }
        self.active = active;
        self.generation = self.generation.wrapping_add(1);
        self.error_message.clear();
        self.status = if self.should_stream() {
            LiveStatsStatus::Streaming
        } else {
            LiveStatsStatus::Idle
        };
        true
    }

    pub fn should_stream(&self) -> bool {
        self.active && self.selection_kind != "none" && !self.targets.is_empty()
    }

    pub fn begin_stream(&mut self) -> Option<(u64, Vec<StatsTarget>)> {
        if !self.should_stream() {
            self.status = LiveStatsStatus::Idle;
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.status = LiveStatsStatus::Streaming;
        self.error_message.clear();
        Some((self.generation, self.targets.clone()))
    }

    pub fn apply_sample(
        &mut self,
        generation: u64,
        container_id: &str,
        sample: StatsReading,
    ) -> bool {
        if generation != self.generation
            || !self.should_stream()
            || !self.targets.iter().any(|target| target.id == container_id)
        {
            return false;
        }
        self.latest.insert(container_id.to_string(), sample);
        let aggregate = self.aggregate();
        let Some(sampled_at) = aggregate.sampled_at else {
            return false;
        };
        let point = StatsHistoryPoint {
            sampled_at,
            cpu_percent: aggregate.cpu_percent,
            memory_raw_bytes: aggregate.memory_raw_bytes,
            memory_working_set_bytes: aggregate.memory_working_set_bytes,
            network_rx_bytes: aggregate.network_rx_bytes,
            network_tx_bytes: aggregate.network_tx_bytes,
        };
        // Group streams tend to deliver one sample per member in a short
        // burst. Keep one aggregate point per Docker sample second rather
        // than shrinking a 600-point history by the number of members.
        if self
            .history
            .back()
            .is_some_and(|previous| previous.sampled_at.timestamp() == sampled_at.timestamp())
        {
            self.history.pop_back();
        }
        push_ring(&mut self.history, point);
        self.status = LiveStatsStatus::Ready;
        self.error_message.clear();
        true
    }

    pub fn apply_error(&mut self, generation: u64, message: &str) -> bool {
        if generation != self.generation || !self.active {
            return false;
        }
        self.status = LiveStatsStatus::Error;
        self.error_message = message.to_string();
        true
    }

    pub fn aggregate(&self) -> AggregateStats {
        let readings = self.latest.values();
        let mut result = AggregateStats::default();
        let mut working_total = 0u64;
        let mut all_have_working = !self.latest.is_empty();
        for reading in readings {
            result.cpu_percent += finite_non_negative(reading.cpu_percent);
            result.memory_raw_bytes = result
                .memory_raw_bytes
                .saturating_add(reading.memory_raw_bytes);
            result.memory_limit_bytes = result
                .memory_limit_bytes
                .saturating_add(reading.memory_limit_bytes);
            result.network_rx_bytes = result
                .network_rx_bytes
                .saturating_add(reading.network_rx_bytes);
            result.network_tx_bytes = result
                .network_tx_bytes
                .saturating_add(reading.network_tx_bytes);
            result.block_read_bytes = result
                .block_read_bytes
                .saturating_add(reading.block_read_bytes);
            result.block_write_bytes = result
                .block_write_bytes
                .saturating_add(reading.block_write_bytes);
            result.pids = result.pids.saturating_add(reading.pids);
            result.sampled_at = Some(result.sampled_at.map_or(reading.sampled_at, |current| {
                current.max(reading.sampled_at)
            }));
            result.reporting_count += 1;
            match reading.memory_working_set_bytes {
                Some(value) => working_total = working_total.saturating_add(value),
                None => all_have_working = false,
            }
        }
        result.memory_working_set_bytes = all_have_working.then_some(working_total);
        result
    }

    pub fn memory_percent(&self) -> f64 {
        let value = self.aggregate();
        if value.memory_limit_bytes == 0 {
            0.0
        } else {
            value.memory_raw_bytes as f64 / value.memory_limit_bytes as f64 * 100.0
        }
    }

    pub fn reset_samples(&mut self) {
        self.latest.clear();
        self.history.clear();
        self.error_message.clear();
        self.status = if self.should_stream() {
            LiveStatsStatus::Streaming
        } else {
            LiveStatsStatus::Idle
        };
    }

    pub fn shutdown(&mut self) {
        self.active = false;
        self.generation = self.generation.wrapping_add(1);
        self.status = LiveStatsStatus::Idle;
        self.latest.clear();
        self.history.clear();
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
) -> Vec<StatsTarget> {
    if kind == "none" || selection_id.trim().is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        if id.trim().is_empty() {
            continue;
        }
        let running = states
            .get(index)
            .is_some_and(|state| state.eq_ignore_ascii_case("running"));
        if !running || result.iter().any(|target: &StatsTarget| target.id == *id) {
            continue;
        }
        result.push(StatsTarget {
            id: id.clone(),
            name: names
                .get(index)
                .filter(|name| !name.is_empty())
                .cloned()
                .unwrap_or_else(|| id.chars().take(12).collect()),
        });
        if kind == "container" {
            break;
        }
    }
    result
}

fn push_ring(history: &mut VecDeque<StatsHistoryPoint>, point: StatsHistoryPoint) {
    while history.len() >= STATS_HISTORY_CAPACITY {
        history.pop_front();
    }
    history.push_back(point);
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn reading(n: u64) -> StatsReading {
        StatsReading {
            cpu_percent: n as f64,
            memory_raw_bytes: n * 10,
            memory_working_set_bytes: Some(n * 8),
            memory_limit_bytes: 1000,
            network_rx_bytes: n * 2,
            network_tx_bytes: n * 3,
            block_read_bytes: n * 4,
            block_write_bytes: n * 5,
            pids: n,
            sampled_at: Utc.timestamp_opt(n as i64, 0).unwrap(),
        }
    }

    fn select_group(state: &mut ContainerStatsState, count: usize) {
        let ids = (0..count).map(|n| format!("id{n}")).collect::<Vec<_>>();
        let states = (0..count).map(|_| "running".into()).collect::<Vec<_>>();
        let names = (0..count).map(|n| format!("name{n}")).collect::<Vec<_>>();
        state.set_selection("group", "group", &ids, &states, &names);
    }

    #[test]
    fn no_selection_never_streams() {
        let mut state = ContainerStatsState::default();
        state.set_active(true);
        assert!(!state.should_stream());
        assert!(state.begin_stream().is_none());
    }

    #[test]
    fn group_keeps_only_running_targets() {
        let mut state = ContainerStatsState::default();
        state.set_selection(
            "group",
            "g",
            &["a".into(), "b".into(), "c".into()],
            &["running".into(), "exited".into(), "paused".into()],
            &["A".into(), "B".into(), "C".into()],
        );
        assert_eq!(
            state.targets,
            vec![StatsTarget {
                id: "a".into(),
                name: "A".into()
            }]
        );
    }

    #[test]
    fn group_retains_every_running_target_for_bounded_bridge_scheduling() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 12);
        assert_eq!(state.targets.len(), 12);
    }

    #[test]
    fn duplicate_targets_are_removed() {
        let mut state = ContainerStatsState::default();
        state.set_selection(
            "group",
            "g",
            &["a".into(), "a".into()],
            &["running".into(), "running".into()],
            &["A".into(), "Again".into()],
        );
        assert_eq!(state.targets.len(), 1);
    }

    #[test]
    fn selection_change_advances_generation_and_clears_history() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        assert!(state.apply_sample(generation, "id0", reading(1)));
        let old = state.generation;
        state.set_selection(
            "container",
            "other",
            &["other".into()],
            &["running".into()],
            &["Other".into()],
        );
        assert!(state.generation > old);
        assert!(state.history.is_empty());
        assert!(state.latest.is_empty());
    }

    #[test]
    fn leaving_tab_invalidates_generation_immediately() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        state.set_active(false);
        assert!(!state.apply_sample(generation, "id0", reading(1)));
        assert_eq!(state.status, LiveStatsStatus::Idle);
    }

    #[test]
    fn stale_and_unknown_samples_are_rejected() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        assert!(!state.apply_sample(generation.wrapping_sub(1), "id0", reading(1)));
        assert!(!state.apply_sample(generation, "other", reading(1)));
    }

    #[test]
    fn group_aggregate_sums_all_current_metrics() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 2);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        state.apply_sample(generation, "id0", reading(2));
        state.apply_sample(generation, "id1", reading(3));
        let aggregate = state.aggregate();
        assert_eq!(aggregate.cpu_percent, 5.0);
        assert_eq!(aggregate.memory_raw_bytes, 50);
        assert_eq!(aggregate.memory_working_set_bytes, Some(40));
        assert_eq!(aggregate.network_rx_bytes, 10);
        assert_eq!(aggregate.block_write_bytes, 25);
        assert_eq!(aggregate.pids, 5);
        assert_eq!(aggregate.reporting_count, 2);
    }

    #[test]
    fn missing_working_set_is_never_labeled_as_raw() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        let mut value = reading(1);
        value.memory_working_set_bytes = None;
        state.apply_sample(generation, "id0", value);
        assert_eq!(state.aggregate().memory_working_set_bytes, None);
    }

    #[test]
    fn memory_percent_uses_raw_usage_semantics() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        state.apply_sample(generation, "id0", reading(5));
        assert_eq!(state.memory_percent(), 5.0);
    }

    #[test]
    fn history_is_a_six_hundred_sample_ring() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        for n in 1..=750 {
            assert!(state.apply_sample(generation, "id0", reading(n)));
        }
        assert_eq!(state.history.len(), STATS_HISTORY_CAPACITY);
        assert_eq!(state.history.front().unwrap().cpu_percent, 151.0);
        assert_eq!(state.history.back().unwrap().cpu_percent, 750.0);
    }

    #[test]
    fn invalid_floating_values_are_sanitized() {
        let stats = ContainerStats {
            cpu_percent: f64::NAN,
            ..Default::default()
        };
        assert_eq!(StatsReading::from(stats).cpu_percent, 0.0);
    }

    #[test]
    fn stale_errors_do_not_replace_current_state() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        assert!(!state.apply_error(generation - 1, "old"));
        assert!(state.error_message.is_empty());
        assert!(state.apply_error(generation, "current"));
        assert_eq!(state.error_message, "current");
    }

    #[test]
    fn shutdown_invalidates_and_drops_samples() {
        let mut state = ContainerStatsState::default();
        select_group(&mut state, 1);
        state.set_active(true);
        let (generation, _) = state.begin_stream().unwrap();
        state.apply_sample(generation, "id0", reading(1));
        let old = state.generation;
        state.shutdown();
        assert!(state.generation > old);
        assert!(!state.active);
        assert!(state.history.is_empty());
    }
}
