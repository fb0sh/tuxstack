//! Pure state/controller logic for Docker networks.

use std::cmp::Ordering;

use tuxstack_docker_core::{DockerError, NetworkDetail, NetworkSummary};

use crate::models::network_model::{NetworkDetailView, NetworkRow};

/// List-level state. Inspect failures never replace this state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum NetworksListState {
    #[default]
    Loading = 0,
    Ready = 1,
    Empty = 2,
    Error = 3,
}

/// Detail-level state. Refresh failures never become detail failures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum NetworkDetailState {
    #[default]
    None = 0,
    Loading = 1,
    Ready = 2,
    Error = 3,
}

/// Numeric values are stable for a future QML bridge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum NetworkSortMode {
    #[default]
    NameAscending = 0,
    NameDescending = 1,
    NewestFirst = 2,
    OldestFirst = 3,
    Driver = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkOperationKind {
    Create,
    Remove,
}

/// Create/remove state is independent of list and detail loading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkOperationState {
    pub active: bool,
    pub kind: Option<NetworkOperationKind>,
    pub network_id: String,
    pub generation: u64,
    pub error_kind: String,
    pub error_message: String,
}

/// Pure, Qt-free state machine backing a network list model.
#[derive(Debug, Clone)]
pub struct NetworksState {
    pub source_rows: Vec<NetworkRow>,
    pub visible_rows: Vec<NetworkRow>,
    pub search_query: String,
    pub sort_mode: NetworkSortMode,
    pub selected_network_id: String,
    pub detail: Option<NetworkDetailView>,
    pub refresh_generation: u64,
    pub detail_generation: u64,
    pub preferred_network_id: String,
    pub initialized: bool,
    pub status: NetworksListState,
    pub status_text: String,
    pub error_kind: String,
    pub detail_status: NetworkDetailState,
    pub detail_error: String,
    pub detail_error_kind: String,
    pub operation: NetworkOperationState,
}

impl Default for NetworksState {
    fn default() -> Self {
        Self {
            source_rows: vec![],
            visible_rows: vec![],
            search_query: String::new(),
            sort_mode: NetworkSortMode::default(),
            selected_network_id: String::new(),
            detail: None,
            refresh_generation: 0,
            detail_generation: 0,
            preferred_network_id: String::new(),
            initialized: false,
            status: NetworksListState::Loading,
            status_text: String::new(),
            error_kind: String::new(),
            detail_status: NetworkDetailState::None,
            detail_error: String::new(),
            detail_error_kind: String::new(),
            operation: NetworkOperationState::default(),
        }
    }
}

impl NetworksState {
    /// Mark the controller initialized. Returns true exactly once.
    pub fn initialize(&mut self) -> bool {
        if self.initialized {
            return false;
        }
        self.initialized = true;
        true
    }

    /// Start a list refresh and invalidate every in-flight inspect.
    pub fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.invalidate_detail(NetworkDetailState::None);
        self.status = NetworksListState::Loading;
        self.status_text.clear();
        self.error_kind.clear();
        self.refresh_generation
    }

    pub fn apply_list(&mut self, generation: u64, summaries: &[NetworkSummary]) -> bool {
        if generation != self.refresh_generation {
            return false;
        }

        let previous = self.selected_network_id.clone();
        self.source_rows = summaries.iter().map(NetworkRow::from).collect();
        self.rebuild_visible();
        self.status = if self.source_rows.is_empty() {
            NetworksListState::Empty
        } else {
            NetworksListState::Ready
        };
        self.status_text.clear();
        self.error_kind.clear();

        let preferred = std::mem::take(&mut self.preferred_network_id);
        if !preferred.is_empty() && self.row_exists(&preferred) {
            self.set_refresh_selection(preferred);
        } else if !previous.is_empty() && self.row_exists(&previous) {
            self.set_refresh_selection(previous);
        } else if let Some(first) = self.visible_rows.first() {
            self.set_refresh_selection(first.network_id.clone());
        } else {
            self.clear_selection();
        }
        true
    }

    pub fn apply_list_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        let error = friendly_network_error(error, NetworkErrorContext::List);
        self.source_rows.clear();
        self.visible_rows.clear();
        self.clear_selection();
        self.status = NetworksListState::Error;
        self.error_kind = error.kind.to_string();
        self.status_text = error.message.to_string();
        true
    }

    /// Prefer a newly-created full ID on the next successful refresh.
    pub fn prefer_created_network(&mut self, network_id: &str) {
        self.preferred_network_id = network_id.to_string();
    }

    pub fn set_search_query(&mut self, query: &str) {
        let previous = self.selected_network_id.clone();
        self.search_query = query.trim().to_string();
        self.rebuild_visible();
        if self.visible_row_exists(&previous) {
            return;
        }
        if let Some(first) = self.visible_rows.first() {
            self.set_refresh_selection(first.network_id.clone());
        } else {
            self.clear_selection();
        }
    }

    pub fn set_sort_mode(&mut self, mode: NetworkSortMode) {
        self.sort_mode = mode;
        self.rebuild_visible();
    }

    /// Select a list row and return the generation required by its inspect.
    pub fn select(&mut self, network_id: &str) -> Option<u64> {
        if !self.row_exists(network_id) {
            return None;
        }
        self.selected_network_id = network_id.to_string();
        Some(self.start_selected_inspect())
    }

    /// Begin or retry inspection of the selected row only.
    pub fn begin_selected_inspect(&mut self) -> Option<u64> {
        if self.selected_network_id.is_empty() || !self.row_exists(&self.selected_network_id) {
            return None;
        }
        Some(self.start_selected_inspect())
    }

    pub fn clear_selection(&mut self) {
        self.selected_network_id.clear();
        self.invalidate_detail(NetworkDetailState::None);
    }

    pub fn apply_detail(&mut self, generation: u64, detail: &NetworkDetail) -> bool {
        if generation != self.detail_generation
            || detail.summary.id != self.selected_network_id
            || !self.row_exists(&detail.summary.id)
        {
            return false;
        }

        // Inspect may contain richer summary fields than list. Keep the row
        // useful for local filtering without inspecting any unselected row.
        if let Some(row) = self
            .source_rows
            .iter_mut()
            .find(|row| row.network_id == detail.summary.id)
        {
            *row = NetworkRow::from(&detail.summary);
        }
        self.rebuild_visible();
        self.detail = Some(NetworkDetailView::from(detail));
        self.detail_status = NetworkDetailState::Ready;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        true
    }

    pub fn apply_detail_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.detail_generation || self.selected_network_id.is_empty() {
            return false;
        }
        let error = friendly_network_error(error, NetworkErrorContext::Detail);
        self.detail = None;
        self.detail_status = NetworkDetailState::Error;
        self.detail_error_kind = error.kind.to_string();
        self.detail_error = error.message.to_string();
        true
    }

    pub fn begin_create(&mut self) -> Option<u64> {
        self.begin_operation(NetworkOperationKind::Create, "")
    }

    pub fn begin_remove(&mut self, network_id: &str) -> Option<u64> {
        if !self.row_exists(network_id) {
            return None;
        }
        self.begin_operation(NetworkOperationKind::Remove, network_id)
    }

    /// Complete creation and remember the daemon-returned full ID.
    pub fn finish_create(&mut self, generation: u64, network_id: &str) -> bool {
        if !self.operation_matches(generation, NetworkOperationKind::Create, "") {
            return false;
        }
        self.operation.active = false;
        self.operation.kind = None;
        self.operation.network_id.clear();
        self.prefer_created_network(network_id);
        // A newly created network must be visible when the post-create refresh
        // selects it; retaining an unrelated local filter would produce a
        // selected detail with no corresponding visible row.
        self.search_query.clear();
        self.rebuild_visible();
        true
    }

    pub fn finish_remove(&mut self, generation: u64, network_id: &str) -> bool {
        if !self.operation_matches(generation, NetworkOperationKind::Remove, network_id) {
            return false;
        }
        self.operation.active = false;
        self.operation.kind = None;
        self.operation.network_id.clear();
        true
    }

    pub fn fail_operation(&mut self, generation: u64, error: &DockerError) -> bool {
        if !self.operation.active || generation != self.operation.generation {
            return false;
        }
        let error = friendly_network_error(error, NetworkErrorContext::Operation);
        self.operation.active = false;
        self.operation.kind = None;
        self.operation.network_id.clear();
        self.operation.error_kind = error.kind.to_string();
        self.operation.error_message = error.message.to_string();
        true
    }

    /// Remove a successfully deleted row and select its next neighbour, then
    /// its previous neighbour, or leave a truly blank detail when none exists.
    /// The returned token is the neighbour's selected-only inspect generation.
    pub fn remove_local(&mut self, network_id: &str) -> Option<u64> {
        let old_visible_index = self
            .visible_rows
            .iter()
            .position(|row| row.network_id == network_id)
            .unwrap_or(0);
        self.source_rows.retain(|row| row.network_id != network_id);
        self.rebuild_visible();

        let generation = if self.selected_network_id == network_id {
            let neighbour = self
                .visible_rows
                .get(old_visible_index.min(self.visible_rows.len().saturating_sub(1)))
                .map(|row| row.network_id.clone());
            if let Some(neighbour) = neighbour {
                self.selected_network_id = neighbour;
                Some(self.start_selected_inspect())
            } else {
                self.clear_selection();
                None
            }
        } else {
            None
        };

        self.status = if self.source_rows.is_empty() {
            NetworksListState::Empty
        } else {
            NetworksListState::Ready
        };
        generation
    }

    pub fn total_network_count(&self) -> usize {
        self.source_rows.len()
    }

    fn begin_operation(&mut self, kind: NetworkOperationKind, network_id: &str) -> Option<u64> {
        if self.operation.active {
            return None;
        }
        self.operation.generation = self.operation.generation.wrapping_add(1);
        self.operation.active = true;
        self.operation.kind = Some(kind);
        self.operation.network_id = network_id.to_string();
        self.operation.error_kind.clear();
        self.operation.error_message.clear();
        Some(self.operation.generation)
    }

    fn operation_matches(
        &self,
        generation: u64,
        kind: NetworkOperationKind,
        network_id: &str,
    ) -> bool {
        self.operation.active
            && self.operation.generation == generation
            && self.operation.kind == Some(kind)
            && self.operation.network_id == network_id
    }

    fn start_selected_inspect(&mut self) -> u64 {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        self.detail = None;
        self.detail_status = NetworkDetailState::Loading;
        self.detail_error.clear();
        self.detail_error_kind.clear();
        self.detail_generation
    }

    fn invalidate_detail(&mut self, state: NetworkDetailState) {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        self.detail = None;
        self.detail_status = state;
        self.detail_error.clear();
        self.detail_error_kind.clear();
    }

    fn set_refresh_selection(&mut self, network_id: String) {
        self.selected_network_id = network_id;
        self.detail = None;
        self.detail_status = NetworkDetailState::None;
        self.detail_error.clear();
        self.detail_error_kind.clear();
    }

    fn row_exists(&self, network_id: &str) -> bool {
        self.source_rows
            .iter()
            .any(|row| row.network_id == network_id)
    }

    fn visible_row_exists(&self, network_id: &str) -> bool {
        !network_id.is_empty()
            && self
                .visible_rows
                .iter()
                .any(|row| row.network_id == network_id)
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

fn row_matches(row: &NetworkRow, query: &str) -> bool {
    row.name.to_lowercase().contains(query)
        || row.network_id.to_lowercase().contains(query)
        || row.short_id.to_lowercase().contains(query)
        || row.driver.to_lowercase().contains(query)
        || row.subnet.to_lowercase().contains(query)
        || row.gateway.to_lowercase().contains(query)
        || row.labels.iter().any(|(key, value)| {
            key.to_lowercase().contains(query) || value.to_lowercase().contains(query)
        })
}

fn compare_rows(left: &NetworkRow, right: &NetworkRow, mode: NetworkSortMode) -> Ordering {
    let name = || {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.network_id.cmp(&right.network_id))
    };
    match mode {
        NetworkSortMode::NameAscending => name(),
        NetworkSortMode::NameDescending => name().reverse(),
        NetworkSortMode::NewestFirst => {
            compare_optional_dates(left.created_at, right.created_at, true).then_with(name)
        }
        NetworkSortMode::OldestFirst => {
            compare_optional_dates(left.created_at, right.created_at, false).then_with(name)
        }
        NetworkSortMode::Driver => left
            .driver
            .to_lowercase()
            .cmp(&right.driver.to_lowercase())
            .then_with(name),
    }
}

fn compare_optional_dates(
    left: Option<chrono::DateTime<chrono::Utc>>,
    right: Option<chrono::DateTime<chrono::Utc>>,
    newest_first: bool,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) if newest_first => right.cmp(&left),
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[derive(Debug, Clone, Copy)]
enum NetworkErrorContext {
    List,
    Detail,
    Operation,
}

#[derive(Debug, Clone, Copy)]
struct FriendlyError {
    kind: &'static str,
    message: &'static str,
}

/// Do not expose daemon payloads, socket paths, or internal error chains in the
/// network page. Detailed errors remain available to the caller for logging.
fn friendly_network_error(error: &DockerError, context: NetworkErrorContext) -> FriendlyError {
    match error {
        DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => FriendlyError {
            kind: "docker_unavailable",
            message: "Docker Engine is not available. Check that Docker is running and try again.",
        },
        DockerError::PermissionDenied => FriendlyError {
            kind: "permission_denied",
            message: "Permission denied while accessing Docker networks. Check Docker socket permissions.",
        },
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => FriendlyError {
            kind: "timeout",
            message: "The Docker network request timed out. Try again.",
        },
        DockerError::NetworkNotFound(_) => FriendlyError {
            kind: "network_not_found",
            message: "This network no longer exists. Refresh the network list.",
        },
        DockerError::NetworkProtected(_) => FriendlyError {
            kind: "network_protected",
            message: "Docker-managed networks such as bridge, host, and none cannot be removed.",
        },
        DockerError::NetworkInUse(_) | DockerError::Conflict(_) => FriendlyError {
            kind: "network_in_use",
            message: "Network is currently in use. Remove containers or disconnect them first.",
        },
        DockerError::InvalidNetworkConfig(_) | DockerError::InvalidResponse(_) => FriendlyError {
            kind: "invalid_network_config",
            message: "Docker returned an invalid network configuration.",
        },
        _ if matches!(context, NetworkErrorContext::List) => FriendlyError {
            kind: "docker",
            message: "Could not load Docker networks. Try again.",
        },
        _ if matches!(context, NetworkErrorContext::Detail) => FriendlyError {
            kind: "docker",
            message: "Could not load network details. Try again.",
        },
        _ => FriendlyError {
            kind: "invalid_network_config",
            message: "Docker rejected the network request. Check the network configuration and try again.",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use tuxstack_docker_core::{NetworkContainer, NetworkIpam, NetworkSubnet};

    use super::*;

    fn summary(id: &str, name: &str, driver: &str, day: Option<u32>) -> NetworkSummary {
        NetworkSummary {
            id: format!("full-{id}"),
            short_id: id.into(),
            name: name.into(),
            driver: driver.into(),
            scope: "local".into(),
            created_at: day.map(|day| Utc.with_ymd_and_hms(2026, 7, day, 0, 0, 0).unwrap()),
            subnet: Some(format!("172.{day:?}.0.0/16")),
            gateway: Some(format!("172.{day:?}.0.1")),
            internal: false,
            attachable: true,
            ingress: false,
            ipv4: true,
            ipv6: false,
            labels: BTreeMap::new(),
        }
    }

    fn detail(source: NetworkSummary) -> NetworkDetail {
        NetworkDetail {
            internal: source.internal,
            attachable: source.attachable,
            ingress: source.ingress,
            summary: source,
            options: BTreeMap::new(),
            ipam: NetworkIpam {
                driver: Some("default".into()),
                options: BTreeMap::new(),
                subnets: vec![NetworkSubnet {
                    subnet: "172.20.0.0/16".into(),
                    gateway: Some("172.20.0.1".into()),
                    ip_range: None,
                    auxiliary_addresses: BTreeMap::new(),
                }],
            },
            containers: vec![NetworkContainer {
                id: "container-full".into(),
                short_id: "container".into(),
                name: "web".into(),
                endpoint_id: "endpoint".into(),
                ipv4_address: Some("172.20.0.2/16".into()),
                ipv6_address: None,
                mac_address: None,
            }],
        }
    }

    fn ready_state() -> NetworksState {
        let mut state = NetworksState::default();
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("z", "Zulu", "bridge", Some(1)),
                summary("a", "alpha", "macvlan", Some(3)),
                summary("b", "Beta", "bridge", Some(2)),
                summary("u", "unknown", "overlay", None),
            ],
        ));
        state
    }

    #[test]
    fn initialize_is_idempotent_and_states_are_independent() {
        let mut state = NetworksState::default();
        assert_eq!(state.status, NetworksListState::Loading);
        assert_eq!(state.detail_status, NetworkDetailState::None);
        assert!(state.initialize());
        assert!(!state.initialize());

        let generation = state.begin_refresh();
        assert!(state.apply_list(generation, &[summary("a", "alpha", "bridge", Some(1))]));
        let detail_generation = state.begin_selected_inspect().unwrap();
        assert!(state.apply_detail_error(detail_generation, &DockerError::OperationTimeout));
        assert_eq!(state.status, NetworksListState::Ready);
        assert_eq!(state.detail_status, NetworkDetailState::Error);
        assert_eq!(state.detail_error_kind, "timeout");
    }

    #[test]
    fn list_transitions_loading_ready_empty_error_and_rejects_stale_results() {
        let mut state = NetworksState::default();
        let stale = state.begin_refresh();
        let current = state.begin_refresh();
        assert!(!state.apply_list(stale, &[summary("x", "old", "bridge", Some(1))]));
        assert!(state.apply_list(current, &[summary("a", "alpha", "bridge", Some(1))]));
        assert_eq!(state.status, NetworksListState::Ready);

        let generation = state.begin_refresh();
        assert!(state.apply_list(generation, &[]));
        assert_eq!(state.status, NetworksListState::Empty);
        assert!(state.selected_network_id.is_empty());

        let generation = state.begin_refresh();
        assert!(state.apply_list_error(generation, &DockerError::EngineUnavailable));
        assert_eq!(state.status, NetworksListState::Error);
        assert_eq!(state.error_kind, "docker_unavailable");
        assert!(!state.status_text.contains("/var/run"));
    }

    #[test]
    fn initial_selection_is_first_visible_and_refresh_preserves_it() {
        let mut state = ready_state();
        assert_eq!(state.visible_rows[0].name, "alpha");
        assert_eq!(state.selected_network_id, "full-a");
        assert_eq!(state.detail_status, NetworkDetailState::None);

        let generation = state.select("full-b").unwrap();
        assert_eq!(state.detail_status, NetworkDetailState::Loading);
        let refresh = state.begin_refresh();
        assert!(state.detail_generation > generation);
        assert!(state.apply_list(
            refresh,
            &[
                summary("b", "Beta", "bridge", Some(2)),
                summary("a", "alpha", "macvlan", Some(3)),
            ]
        ));
        assert_eq!(state.selected_network_id, "full-b");
        assert_eq!(state.detail_status, NetworkDetailState::None);
    }

    #[test]
    fn preferred_created_id_wins_once_then_previous_selection_is_used() {
        let mut state = ready_state();
        state.selected_network_id = "full-z".into();
        state.prefer_created_network("full-new");
        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("z", "Zulu", "bridge", Some(1)),
                summary("new", "Created", "bridge", Some(4)),
            ]
        ));
        assert_eq!(state.selected_network_id, "full-new");
        assert!(state.preferred_network_id.is_empty());

        let generation = state.begin_refresh();
        assert!(state.apply_list(
            generation,
            &[
                summary("new", "Created", "bridge", Some(4)),
                summary("z", "Zulu", "bridge", Some(1)),
            ]
        ));
        assert_eq!(state.selected_network_id, "full-new");
    }

    #[test]
    fn search_is_trimmed_local_case_insensitive_and_complete() {
        let mut labelled = summary("abcdef123456", "frontend", "bridge", Some(1));
        labelled.subnet = Some("10.42.0.0/16".into());
        labelled.gateway = Some("10.42.0.1".into());
        labelled.labels = BTreeMap::from([("com.example.Tier".into(), "Edge".into())]);
        let mut state = NetworksState::default();
        let generation = state.begin_refresh();
        state.apply_list(
            generation,
            &[labelled, summary("other", "backend", "overlay", Some(2))],
        );

        for query in [
            " FRONT ",
            "full-abcdef",
            "ABCDEF123456",
            "BRIDGE",
            "10.42.0.0",
            "10.42.0.1",
            "tier",
            "EDGE",
        ] {
            state.set_search_query(query);
            assert_eq!(state.visible_rows.len(), 1, "query {query}");
            assert_eq!(state.visible_rows[0].name, "frontend");
            assert_eq!(state.selected_network_id, "full-abcdef123456");
        }
        state.set_search_query("missing");
        assert!(state.visible_rows.is_empty());
        assert!(state.selected_network_id.is_empty());
        assert_eq!(state.detail_status, NetworkDetailState::None);
    }

    #[test]
    fn all_sort_modes_have_deterministic_order_and_unknown_dates_are_last() {
        let mut state = ready_state();
        let expected = [
            vec!["alpha", "Beta", "unknown", "Zulu"],
            vec!["Zulu", "unknown", "Beta", "alpha"],
            vec!["alpha", "Beta", "Zulu", "unknown"],
            vec!["Zulu", "Beta", "alpha", "unknown"],
            vec!["Beta", "Zulu", "alpha", "unknown"],
        ];
        for (mode, names) in [
            NetworkSortMode::NameAscending,
            NetworkSortMode::NameDescending,
            NetworkSortMode::NewestFirst,
            NetworkSortMode::OldestFirst,
            NetworkSortMode::Driver,
        ]
        .into_iter()
        .zip(expected)
        {
            state.set_sort_mode(mode);
            assert_eq!(
                state
                    .visible_rows
                    .iter()
                    .map(|row| row.name.as_str())
                    .collect::<Vec<_>>(),
                names
            );
        }
    }

    #[test]
    fn inspect_is_selected_only_and_rejects_stale_or_wrong_id_results() {
        let mut state = ready_state();
        assert!(state.select("does-not-exist").is_none());
        let generation_a = state.select("full-a").unwrap();
        let generation_b = state.select("full-b").unwrap();
        assert_ne!(generation_a, generation_b);
        assert!(!state.apply_detail(
            generation_a,
            &detail(summary("a", "alpha", "macvlan", Some(3)))
        ));
        assert!(!state.apply_detail(
            generation_b,
            &detail(summary("a", "alpha", "macvlan", Some(3)))
        ));
        assert!(state.apply_detail(
            generation_b,
            &detail(summary("b", "Beta", "bridge", Some(2)))
        ));
        assert_eq!(state.detail_status, NetworkDetailState::Ready);
        assert_eq!(state.detail.as_ref().unwrap().network_id, "full-b");
    }

    #[test]
    fn selected_removal_chooses_next_then_previous_then_blank() {
        let mut state = NetworksState::default();
        let generation = state.begin_refresh();
        state.apply_list(
            generation,
            &[
                summary("a", "alpha", "bridge", Some(1)),
                summary("b", "beta", "bridge", Some(2)),
                summary("c", "charlie", "bridge", Some(3)),
            ],
        );
        state.select("full-b");
        let inspect = state.remove_local("full-b").unwrap();
        assert_eq!(state.selected_network_id, "full-c");
        assert_eq!(state.detail_generation, inspect);
        assert_eq!(state.detail_status, NetworkDetailState::Loading);

        state.remove_local("full-c");
        assert_eq!(state.selected_network_id, "full-a");
        state.remove_local("full-a");
        assert!(state.selected_network_id.is_empty());
        assert_eq!(state.detail_status, NetworkDetailState::None);
        assert_eq!(state.status, NetworksListState::Empty);
    }

    #[test]
    fn removing_unselected_row_preserves_detail_and_selection() {
        let mut state = ready_state();
        let generation = state.select("full-a").unwrap();
        let selected_detail = detail(summary("a", "alpha", "macvlan", Some(3)));
        assert!(state.apply_detail(generation, &selected_detail));
        assert!(state.remove_local("full-b").is_none());
        assert_eq!(state.selected_network_id, "full-a");
        assert_eq!(state.detail_status, NetworkDetailState::Ready);
        assert!(state.detail.is_some());
    }

    #[test]
    fn create_remove_operations_are_exclusive_and_generation_checked() {
        let mut state = ready_state();
        let create = state.begin_create().unwrap();
        assert!(state.begin_create().is_none());
        assert!(state.begin_remove("full-a").is_none());
        assert!(!state.finish_create(create.wrapping_sub(1), "full-new"));
        state.set_search_query("Zulu");
        assert!(state.finish_create(create, "full-new"));
        assert_eq!(state.preferred_network_id, "full-new");
        assert!(state.search_query.is_empty());
        assert_eq!(state.visible_rows.len(), state.source_rows.len());
        assert!(!state.operation.active);

        let remove = state.begin_remove("full-a").unwrap();
        assert!(!state.finish_remove(remove, "full-b"));
        assert!(state.finish_remove(remove, "full-a"));
        assert!(state.begin_remove("missing").is_none());
    }

    #[test]
    fn operation_errors_are_safe_specific_and_stale_safe() {
        let mut state = ready_state();
        let generation = state.begin_remove("full-a").unwrap();
        assert!(!state.fail_operation(
            generation.wrapping_sub(1),
            &DockerError::Conflict("raw daemon payload".into())
        ));
        assert!(state.operation.active);
        assert!(state.fail_operation(
            generation,
            &DockerError::Conflict("raw daemon payload".into())
        ));
        assert_eq!(state.operation.error_kind, "network_in_use");
        assert_eq!(
            state.operation.error_message,
            "Network is currently in use. Remove containers or disconnect them first."
        );

        let generation = state.begin_remove("full-a").unwrap();
        assert!(state.fail_operation(
            generation,
            &DockerError::NetworkProtected("raw daemon payload".into())
        ));
        assert_eq!(state.operation.error_kind, "network_protected");
        assert!(!state.operation.error_message.contains("raw daemon payload"));
        assert!(!state.operation.error_message.contains("raw daemon"));

        let generation = state.begin_create().unwrap();
        assert!(state.fail_operation(
            generation,
            &DockerError::Api("private daemon details".into())
        ));
        assert_eq!(state.operation.error_kind, "invalid_network_config");
        assert!(!state.operation.error_message.contains("private"));
    }

    #[test]
    fn known_errors_have_friendly_detail_kinds() {
        let cases = [
            (DockerError::PermissionDenied, "permission_denied"),
            (DockerError::OperationTimeout, "timeout"),
            (
                DockerError::NetworkNotFound("secret".into()),
                "network_not_found",
            ),
            (
                DockerError::InvalidResponse("secret".into()),
                "invalid_network_config",
            ),
            (
                DockerError::InvalidNetworkConfig("secret".into()),
                "invalid_network_config",
            ),
        ];
        for (error, kind) in cases {
            let friendly = friendly_network_error(&error, NetworkErrorContext::Detail);
            assert_eq!(friendly.kind, kind);
            assert!(!friendly.message.contains("secret"));
        }
    }
}
