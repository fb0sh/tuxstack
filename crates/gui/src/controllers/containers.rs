//! Pure state machine for the unified Containers list, selection and Info path.
//!
//! This module performs no Docker or Qt work. The bridge owns async requests
//! and cancellation, and can only apply results carrying a current generation.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use tuxstack_docker_core::{
    ContainerDetail, ContainerGroupId, ContainerGroupSummary, ContainerOperationState,
    ContainerRuntimeState, ContainerSelection, ContainerSortMode, ContainerSummary, DockerError,
    GroupOperationState, container_matches_search, group_compose_containers,
};

use crate::models::container_model::{
    ContainerDetailView, ContainerGroupDetailView, ContainerListRow, ContainerRowKind,
    ContainerSection,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ContainersListState {
    #[default]
    Loading = 0,
    Ready = 1,
    Empty = 2,
    Error = 3,
    DockerUnavailable = 4,
    PermissionDenied = 5,
}

impl ContainersListState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Empty => "empty",
            Self::Error => "error",
            Self::DockerUnavailable => "docker_unavailable",
            Self::PermissionDenied => "permission",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum ContainerDetailState {
    #[default]
    None = 0,
    Loading = 1,
    Ready = 2,
    Error = 3,
}

impl ContainerDetailState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    None,
    LoadContainer { generation: u64 },
    GroupReady { generation: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationToken {
    pub generation: u64,
    pub operation: ContainerOperationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTargetResult {
    pub container_id: String,
    pub container_name: String,
    pub success: bool,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOperationResult {
    pub operation: GroupOperationState,
    pub targets: Vec<GroupTargetResult>,
}

impl GroupOperationResult {
    pub fn success_count(&self) -> usize {
        self.targets.iter().filter(|target| target.success).count()
    }

    pub fn failure_count(&self) -> usize {
        self.targets.len().saturating_sub(self.success_count())
    }

    pub fn message(&self) -> String {
        let verb = match self.operation {
            GroupOperationState::Starting => "Started",
            GroupOperationState::Stopping => "Stopped",
            GroupOperationState::Restarting => "Restarted",
            GroupOperationState::Pausing => "Paused",
            GroupOperationState::Unpausing => "Resumed",
            GroupOperationState::Removing => "Removed",
            GroupOperationState::Idle => "Updated",
        };
        let mut message = format!("{verb} {} containers.", self.success_count());
        let failures = self
            .targets
            .iter()
            .filter(|target| !target.success)
            .map(|target| format!("{} — {}", target.container_name, target.error))
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            message.push_str(&format!(
                " {} failed: {}",
                failures.len(),
                failures.join("; ")
            ));
        }
        message
    }
}

/// Single authoritative state for list, selection, details and typed busy
/// operations. QML receives only derived `selectionKind` and `selectionId`.
#[derive(Debug, Clone)]
pub struct ContainersState {
    pub endpoint_key: String,
    pub source_rows: Vec<ContainerSummary>,
    pub groups: Vec<ContainerGroupSummary>,
    pub visible_rows: Vec<ContainerListRow>,
    pub expanded_groups: HashSet<ContainerGroupId>,
    pub search_query: String,
    pub sort_mode: ContainerSortMode,
    pub selection: ContainerSelection,
    pub selection_generation: u64,
    pub refresh_generation: u64,
    pub operation_generation: u64,
    pub initialized: bool,
    pub refresh_in_progress: bool,
    pub using_cache: bool,
    pub list_state: ContainersListState,
    pub list_error_kind: String,
    pub list_error_message: String,
    pub detail_state: ContainerDetailState,
    pub detail_error_kind: String,
    pub detail_error_message: String,
    pub container_detail: Option<ContainerDetailView>,
    pub group_detail: Option<ContainerGroupDetailView>,
    pub operations: HashMap<String, OperationToken>,
    pub group_operations: HashMap<ContainerGroupId, (u64, GroupOperationState)>,
    pub last_group_result: Option<GroupOperationResult>,
    /// Future tab controllers can key their reset against this value. The
    /// current Info-only path always resets it to `info` on selection change.
    pub active_tab: String,
}

impl Default for ContainersState {
    fn default() -> Self {
        Self::new("local")
    }
}

impl ContainersState {
    pub fn new(endpoint_key: impl Into<String>) -> Self {
        Self {
            endpoint_key: endpoint_key.into(),
            source_rows: Vec::new(),
            groups: Vec::new(),
            visible_rows: Vec::new(),
            expanded_groups: HashSet::new(),
            search_query: String::new(),
            sort_mode: ContainerSortMode::RunningFirst,
            selection: ContainerSelection::None,
            selection_generation: 0,
            refresh_generation: 0,
            operation_generation: 0,
            initialized: false,
            refresh_in_progress: false,
            using_cache: false,
            list_state: ContainersListState::Loading,
            list_error_kind: String::new(),
            list_error_message: String::new(),
            detail_state: ContainerDetailState::None,
            detail_error_kind: String::new(),
            detail_error_message: String::new(),
            container_detail: None,
            group_detail: None,
            operations: HashMap::new(),
            group_operations: HashMap::new(),
            last_group_result: None,
            active_tab: "info".to_string(),
        }
    }

    pub fn initialize(&mut self) -> bool {
        if self.initialized {
            return false;
        }
        self.initialized = true;
        true
    }

    pub fn set_endpoint_key(&mut self, endpoint_key: &str) {
        if self.endpoint_key == endpoint_key {
            return;
        }
        self.endpoint_key = endpoint_key.to_string();
        self.expanded_groups.clear();
        self.set_selection(ContainerSelection::None);
        self.groups = group_compose_containers(&self.endpoint_key, &self.source_rows);
        self.rebuild_visible();
    }

    pub fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.refresh_in_progress = true;
        self.list_error_kind.clear();
        self.list_error_message.clear();
        if self.source_rows.is_empty() {
            self.list_state = ContainersListState::Loading;
        }
        self.refresh_generation
    }

    /// Apply a stale cache snapshot while the same generation continues its
    /// live request. Existing content is replaced immediately but the bridge
    /// must still call `apply_live` or `apply_list_error`.
    pub fn apply_cached(&mut self, generation: u64, summaries: &[ContainerSummary]) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        self.replace_source(summaries);
        self.using_cache = true;
        self.list_state = if summaries.is_empty() {
            ContainersListState::Loading
        } else {
            ContainersListState::Ready
        };
        true
    }

    pub fn apply_live(&mut self, generation: u64, summaries: &[ContainerSummary]) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        self.replace_source(summaries);
        self.using_cache = false;
        self.refresh_in_progress = false;
        self.list_state = if self.source_rows.is_empty() {
            ContainersListState::Empty
        } else {
            ContainersListState::Ready
        };
        self.list_error_kind.clear();
        self.list_error_message.clear();
        true
    }

    pub fn apply_list_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        self.refresh_in_progress = false;
        let (state, kind, message) = friendly_error(error, ErrorContext::List);
        // A useful cached list remains visible if live revalidation fails.
        if self.using_cache && !self.source_rows.is_empty() {
            self.list_state = ContainersListState::Ready;
        } else {
            self.source_rows.clear();
            self.groups.clear();
            self.visible_rows.clear();
            self.set_selection(ContainerSelection::None);
            self.list_state = state;
        }
        self.list_error_kind = kind.to_string();
        self.list_error_message = message;
        true
    }

    pub fn set_search(&mut self, query: &str) {
        self.search_query = query.trim().to_string();
        self.rebuild_visible();
    }

    pub fn set_sort(&mut self, sort: ContainerSortMode) {
        self.sort_mode = sort;
        self.rebuild_visible();
    }

    pub fn toggle_group(&mut self, opaque_group_id: &str) -> bool {
        let Some(group_id) = self.group_id_from_opaque(opaque_group_id) else {
            return false;
        };
        if !self.expanded_groups.remove(&group_id) {
            self.expanded_groups.insert(group_id);
        }
        self.rebuild_visible();
        true
    }

    pub fn select_row(&mut self, opaque_id: &str) -> SelectionAction {
        let Some(row) = self
            .visible_rows
            .iter()
            .find(|row| row.id == opaque_id && row.selectable())
        else {
            return SelectionAction::None;
        };
        let desired = match row.row_kind {
            ContainerRowKind::Group => self
                .group_id_from_opaque(opaque_id)
                .map(|group_id| ContainerSelection::Group { group_id }),
            ContainerRowKind::ContainerChild | ContainerRowKind::Individual => {
                Some(ContainerSelection::Container {
                    container_id: opaque_id.to_string(),
                })
            }
            ContainerRowKind::SectionHeader => None,
        };
        let Some(desired) = desired else {
            return SelectionAction::None;
        };
        let desired = if self.selection == desired {
            ContainerSelection::None
        } else {
            desired
        };
        self.set_selection(desired)
    }

    pub fn select_container(&mut self, container_id: &str) -> SelectionAction {
        if !self.source_rows.iter().any(|row| row.id == container_id) {
            return SelectionAction::None;
        }
        self.set_selection(ContainerSelection::Container {
            container_id: container_id.to_string(),
        })
    }

    pub fn reload_detail(&mut self) -> SelectionAction {
        match self.selection.clone() {
            ContainerSelection::None => SelectionAction::None,
            ContainerSelection::Container { .. }
                if self.detail_state == ContainerDetailState::Loading =>
            {
                SelectionAction::None
            }
            ContainerSelection::Container { .. } => {
                self.reset_selection_states();
                self.detail_state = ContainerDetailState::Loading;
                SelectionAction::LoadContainer {
                    generation: self.selection_generation,
                }
            }
            ContainerSelection::Group { group_id } => {
                self.reset_selection_states();
                self.build_group_detail(&group_id);
                SelectionAction::GroupReady {
                    generation: self.selection_generation,
                }
            }
        }
    }

    pub fn clear_selection(&mut self) -> bool {
        if matches!(self.selection, ContainerSelection::None) {
            return false;
        }
        self.set_selection(ContainerSelection::None);
        true
    }

    pub fn apply_detail(&mut self, generation: u64, detail: &ContainerDetail) -> bool {
        if generation != self.selection_generation
            || !matches!(
                &self.selection,
                ContainerSelection::Container { container_id } if container_id == &detail.summary.id
            )
        {
            return false;
        }
        self.container_detail = Some(ContainerDetailView::from_detail_for_endpoint(
            detail,
            &self.endpoint_key,
        ));
        self.group_detail = None;
        self.detail_state = ContainerDetailState::Ready;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
        true
    }

    pub fn apply_detail_error(&mut self, generation: u64, error: &DockerError) -> bool {
        if generation != self.selection_generation
            || !matches!(self.selection, ContainerSelection::Container { .. })
        {
            return false;
        }
        let (_, kind, message) = friendly_error(error, ErrorContext::Detail);
        self.container_detail = None;
        self.detail_state = ContainerDetailState::Error;
        self.detail_error_kind = kind.to_string();
        self.detail_error_message = message;
        true
    }

    pub fn reveal_environment(&mut self, index: usize) -> bool {
        self.container_detail
            .as_mut()
            .is_some_and(|detail| detail.reveal_environment(index))
    }

    pub fn conceal_environment(&mut self, index: usize) -> bool {
        self.container_detail
            .as_mut()
            .is_some_and(|detail| detail.conceal_environment(index))
    }

    pub fn begin_operation(
        &mut self,
        container_id: &str,
        operation: ContainerOperationState,
    ) -> Option<u64> {
        if operation == ContainerOperationState::Idle
            || !self.source_rows.iter().any(|row| row.id == container_id)
            || self.operations.contains_key(container_id)
        {
            return None;
        }
        self.operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        self.operations.insert(
            container_id.to_string(),
            OperationToken {
                generation,
                operation,
            },
        );
        self.rebuild_visible();
        Some(generation)
    }

    pub fn finish_operation(
        &mut self,
        container_id: &str,
        generation: u64,
        operation: ContainerOperationState,
    ) -> bool {
        if self.operations.get(container_id).copied()
            != Some(OperationToken {
                generation,
                operation,
            })
        {
            return false;
        }
        self.operations.remove(container_id);
        self.rebuild_visible();
        true
    }

    pub fn fail_operation(
        &mut self,
        container_id: &str,
        generation: u64,
        operation: ContainerOperationState,
        error: &DockerError,
    ) -> bool {
        if !self.finish_operation(container_id, generation, operation) {
            return false;
        }
        let (_, kind, message) = friendly_error(error, ErrorContext::Operation);
        self.list_error_kind = kind.to_string();
        self.list_error_message = message;
        true
    }

    pub fn begin_group_operation(
        &mut self,
        opaque_group_id: &str,
        operation: GroupOperationState,
    ) -> Option<(u64, Vec<String>)> {
        if operation == GroupOperationState::Idle {
            return None;
        }
        let group_id = self.group_id_from_opaque(opaque_group_id)?;
        if self.group_operations.contains_key(&group_id) {
            return None;
        }
        let group = self.groups.iter().find(|group| group.id == group_id)?;
        let targets = group
            .containers
            .iter()
            .filter_map(|id| self.source_rows.iter().find(|row| &row.id == id))
            .filter(|row| group_operation_applies(row.state, operation))
            .map(|row| row.id.clone())
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return None;
        }
        self.operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        self.group_operations
            .insert(group_id, (generation, operation));
        self.last_group_result = None;
        self.rebuild_visible();
        Some((generation, targets))
    }

    pub fn finish_group_operation(
        &mut self,
        opaque_group_id: &str,
        generation: u64,
        result: GroupOperationResult,
    ) -> bool {
        let Some(group_id) = self.group_id_from_opaque(opaque_group_id) else {
            return false;
        };
        if self.group_operations.get(&group_id).copied() != Some((generation, result.operation)) {
            return false;
        }
        self.group_operations.remove(&group_id);
        self.last_group_result = Some(result);
        self.rebuild_visible();
        true
    }

    /// Remove one or more successfully deleted containers. If the current
    /// selection disappears, select the adjacent selectable row from the old
    /// flattened list. Group removal never touches volumes, config files, or
    /// project paths; those resources are not represented here.
    pub fn remove_local_many(&mut self, ids: &[String]) -> SelectionAction {
        let removed = ids.iter().map(String::as_str).collect::<HashSet<_>>();
        let old_index = self.selection_row_index().unwrap_or(0);
        let selected_removed = match &self.selection {
            ContainerSelection::Container { container_id } => {
                removed.contains(container_id.as_str())
            }
            ContainerSelection::Group { group_id } => self
                .groups
                .iter()
                .find(|group| &group.id == group_id)
                .is_some_and(|group| {
                    group
                        .containers
                        .iter()
                        .all(|id| removed.contains(id.as_str()))
                }),
            ContainerSelection::None => false,
        };
        self.source_rows
            .retain(|row| !removed.contains(row.id.as_str()));
        for id in ids {
            self.operations.remove(id);
        }
        self.groups = group_compose_containers(&self.endpoint_key, &self.source_rows);
        self.expanded_groups
            .retain(|group_id| self.groups.iter().any(|group| &group.id == group_id));
        self.rebuild_visible();
        self.list_state = if self.source_rows.is_empty() {
            ContainersListState::Empty
        } else {
            ContainersListState::Ready
        };
        if !selected_removed {
            return SelectionAction::None;
        }
        let neighbour = self
            .visible_rows
            .iter()
            .skip(old_index.min(self.visible_rows.len()))
            .chain(
                self.visible_rows[..old_index.min(self.visible_rows.len())]
                    .iter()
                    .rev(),
            )
            .find(|row| row.selectable())
            .map(|row| row.id.clone());
        match neighbour {
            Some(id) => self.select_row(&id),
            None => self.set_selection(ContainerSelection::None),
        }
    }

    pub fn selection_kind(&self) -> &'static str {
        match self.selection {
            ContainerSelection::None => "none",
            ContainerSelection::Group { .. } => "group",
            ContainerSelection::Container { .. } => "container",
        }
    }

    pub fn selection_id(&self) -> String {
        match &self.selection {
            ContainerSelection::None => String::new(),
            ContainerSelection::Group { group_id } => opaque_group_id(group_id),
            ContainerSelection::Container { container_id } => container_id.clone(),
        }
    }

    pub fn total_count(&self) -> usize {
        self.source_rows.len()
    }

    pub fn running_count(&self) -> usize {
        self.source_rows
            .iter()
            .filter(|row| row.state == ContainerRuntimeState::Running)
            .count()
    }

    pub fn paused_count(&self) -> usize {
        self.source_rows
            .iter()
            .filter(|row| row.state == ContainerRuntimeState::Paused)
            .count()
    }

    pub fn stopped_count(&self) -> usize {
        self.total_count()
            .saturating_sub(self.running_count() + self.paused_count())
    }

    fn replace_source(&mut self, summaries: &[ContainerSummary]) {
        self.source_rows = summaries.to_vec();
        self.groups = group_compose_containers(&self.endpoint_key, &self.source_rows);
        self.expanded_groups
            .retain(|group_id| self.groups.iter().any(|group| &group.id == group_id));
        if !self.selection_still_exists() {
            self.set_selection(ContainerSelection::None);
        } else if let ContainerSelection::Group { group_id } = self.selection.clone() {
            self.build_group_detail(&group_id);
        }
        self.rebuild_visible();
    }

    fn selection_still_exists(&self) -> bool {
        match &self.selection {
            ContainerSelection::None => true,
            ContainerSelection::Container { container_id } => {
                self.source_rows.iter().any(|row| &row.id == container_id)
            }
            ContainerSelection::Group { group_id } => {
                self.groups.iter().any(|group| &group.id == group_id)
            }
        }
    }

    fn set_selection(&mut self, selection: ContainerSelection) -> SelectionAction {
        if self.selection == selection {
            return SelectionAction::None;
        }
        self.selection = selection;
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.reset_selection_states();
        let action = match self.selection.clone() {
            ContainerSelection::None => SelectionAction::None,
            ContainerSelection::Container { .. } => {
                self.detail_state = ContainerDetailState::Loading;
                SelectionAction::LoadContainer {
                    generation: self.selection_generation,
                }
            }
            ContainerSelection::Group { group_id } => {
                self.build_group_detail(&group_id);
                SelectionAction::GroupReady {
                    generation: self.selection_generation,
                }
            }
        };
        self.rebuild_visible();
        action
    }

    fn reset_selection_states(&mut self) {
        self.container_detail = None;
        self.group_detail = None;
        self.detail_state = ContainerDetailState::None;
        self.detail_error_kind.clear();
        self.detail_error_message.clear();
        self.active_tab.clear();
        self.active_tab.push_str("info");
    }

    fn build_group_detail(&mut self, group_id: &ContainerGroupId) {
        let Some(group) = self.groups.iter().find(|group| &group.id == group_id) else {
            self.group_detail = None;
            self.detail_state = ContainerDetailState::Error;
            return;
        };
        self.group_detail = Some(ContainerGroupDetailView::from_group(
            opaque_group_id(group_id),
            group,
            &self.source_rows,
        ));
        self.container_detail = None;
        self.detail_state = ContainerDetailState::Ready;
    }

    fn rebuild_visible(&mut self) {
        let query = self.search_query.trim();
        let member_to_group = self
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .containers
                    .iter()
                    .map(move |id| (id.clone(), group.id.clone()))
            })
            .collect::<HashMap<_, _>>();
        let summary_by_id = self
            .source_rows
            .iter()
            .map(|summary| (summary.id.clone(), summary.clone()))
            .collect::<HashMap<_, _>>();

        let mut groups_by_section: HashMap<ContainerSection, Vec<ContainerGroupSummary>> =
            HashMap::new();
        let mut matching_members: HashMap<ContainerGroupId, Vec<ContainerSummary>> = HashMap::new();
        for summary in &self.source_rows {
            let Some(group_id) = member_to_group.get(&summary.id) else {
                continue;
            };
            if query.is_empty() || container_matches_search(summary, query) {
                matching_members
                    .entry(group_id.clone())
                    .or_default()
                    .push(summary.clone());
            }
        }
        for group in &self.groups {
            let group_match = query.is_empty()
                || group
                    .project_name
                    .to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase());
            let members = matching_members.get(&group.id).cloned().unwrap_or_default();
            if group_match || !members.is_empty() {
                if !query.is_empty() && !members.is_empty() {
                    self.expanded_groups.insert(group.id.clone());
                }
                groups_by_section
                    .entry(ContainerSection::from_group(group))
                    .or_default()
                    .push(group.clone());
            }
        }

        let mut individuals_by_section: HashMap<ContainerSection, Vec<ContainerSummary>> =
            HashMap::new();
        for summary in &self.source_rows {
            if member_to_group.contains_key(&summary.id) {
                continue;
            }
            if query.is_empty() || container_matches_search(summary, query) {
                individuals_by_section
                    .entry(ContainerSection::from_state(summary.state))
                    .or_default()
                    .push(summary.clone());
            }
        }

        let mut result = Vec::new();
        for section in self.section_order() {
            let mut items = Vec::<TopLevelItem>::new();
            items.extend(
                groups_by_section
                    .remove(&section)
                    .unwrap_or_default()
                    .into_iter()
                    .map(TopLevelItem::Group),
            );
            items.extend(
                individuals_by_section
                    .remove(&section)
                    .unwrap_or_default()
                    .into_iter()
                    .map(TopLevelItem::Container),
            );
            items.sort_by(|left, right| {
                compare_top_level(left, right, self.sort_mode, &summary_by_id)
            });
            if items.is_empty() {
                continue;
            }
            result.push(ContainerListRow::section_header(section, items.len()));
            for item in items {
                match item {
                    TopLevelItem::Group(group) => {
                        let id = opaque_group_id(&group.id);
                        let selected = matches!(
                            &self.selection,
                            ContainerSelection::Group { group_id } if group_id == &group.id
                        );
                        let operation = self
                            .group_operations
                            .get(&group.id)
                            .map(|(_, operation)| group_operation_name(*operation))
                            .unwrap_or_default()
                            .to_string();
                        let expanded = self.expanded_groups.contains(&group.id);
                        result.push(ContainerListRow::group(
                            &group, id, expanded, selected, operation,
                        ));
                        if expanded {
                            let mut members = if query.is_empty()
                                || group
                                    .project_name
                                    .to_ascii_lowercase()
                                    .contains(&query.to_ascii_lowercase())
                            {
                                group
                                    .containers
                                    .iter()
                                    .filter_map(|id| summary_by_id.get(id).cloned())
                                    .collect::<Vec<_>>()
                            } else {
                                matching_members.get(&group.id).cloned().unwrap_or_default()
                            };
                            sort_member_rows(&mut members, self.sort_mode);
                            for summary in members {
                                let selected = matches!(
                                    &self.selection,
                                    ContainerSelection::Container { container_id }
                                        if container_id == &summary.id
                                );
                                let operation = self
                                    .operations
                                    .get(&summary.id)
                                    .map(|token| token.operation)
                                    .unwrap_or(ContainerOperationState::Idle);
                                result.push(ContainerListRow::container(
                                    &summary,
                                    ContainerRowKind::ContainerChild,
                                    section,
                                    opaque_group_id(&group.id),
                                    selected,
                                    operation,
                                ));
                            }
                        }
                    }
                    TopLevelItem::Container(summary) => {
                        let selected = matches!(
                            &self.selection,
                            ContainerSelection::Container { container_id }
                                if container_id == &summary.id
                        );
                        let operation = self
                            .operations
                            .get(&summary.id)
                            .map(|token| token.operation)
                            .unwrap_or(ContainerOperationState::Idle);
                        result.push(ContainerListRow::container(
                            &summary,
                            ContainerRowKind::Individual,
                            section,
                            String::new(),
                            selected,
                            operation,
                        ));
                    }
                }
            }
        }
        self.visible_rows = result;
    }

    fn section_order(&self) -> Vec<ContainerSection> {
        match self.sort_mode {
            ContainerSortMode::StoppedFirst => vec![
                ContainerSection::Stopped,
                ContainerSection::Paused,
                ContainerSection::Restarting,
                ContainerSection::Running,
            ],
            _ => ContainerSection::DISPLAY_ORDER.to_vec(),
        }
    }

    pub fn group_id_from_opaque(&self, opaque: &str) -> Option<ContainerGroupId> {
        self.groups
            .iter()
            .find(|group| opaque_group_id(&group.id) == opaque)
            .map(|group| group.id.clone())
    }

    fn selection_row_index(&self) -> Option<usize> {
        let id = self.selection_id();
        self.visible_rows.iter().position(|row| row.id == id)
    }
}

#[derive(Debug, Clone)]
enum TopLevelItem {
    Group(ContainerGroupSummary),
    Container(ContainerSummary),
}

fn compare_top_level(
    left: &TopLevelItem,
    right: &TopLevelItem,
    sort: ContainerSortMode,
    summaries: &HashMap<String, ContainerSummary>,
) -> Ordering {
    let left_name = item_name(left).to_ascii_lowercase();
    let right_name = item_name(right).to_ascii_lowercase();
    let name = || {
        left_name
            .cmp(&right_name)
            .then_with(|| item_id(left).cmp(&item_id(right)))
    };
    let group_cmp =
        || matches!(right, TopLevelItem::Group(_)).cmp(&matches!(left, TopLevelItem::Group(_)));
    let individual_cmp =
        || matches!(left, TopLevelItem::Group(_)).cmp(&matches!(right, TopLevelItem::Group(_)));
    match sort {
        ContainerSortMode::NameAscending => name(),
        ContainerSortMode::NameDescending => name().reverse(),
        ContainerSortMode::NewestFirst => item_created(right, summaries)
            .cmp(&item_created(left, summaries))
            .then_with(name),
        ContainerSortMode::OldestFirst => item_created(left, summaries)
            .cmp(&item_created(right, summaries))
            .then_with(name),
        ContainerSortMode::RunningFirst => group_cmp().then_with(name),
        ContainerSortMode::StoppedFirst => group_cmp().then_with(name),
        ContainerSortMode::ComposeGroupsFirst => group_cmp().then_with(name),
        ContainerSortMode::IndividualContainersFirst => individual_cmp().then_with(name),
    }
}

fn item_name(item: &TopLevelItem) -> &str {
    match item {
        TopLevelItem::Group(group) => &group.display_name,
        TopLevelItem::Container(summary) => summary.display_name(),
    }
}

fn item_id(item: &TopLevelItem) -> String {
    match item {
        TopLevelItem::Group(group) => opaque_group_id(&group.id),
        TopLevelItem::Container(summary) => summary.id.clone(),
    }
}

fn item_created(
    item: &TopLevelItem,
    summaries: &HashMap<String, ContainerSummary>,
) -> chrono::DateTime<chrono::Utc> {
    match item {
        TopLevelItem::Container(summary) => summary.created_at,
        TopLevelItem::Group(group) => group
            .containers
            .iter()
            .filter_map(|id| summaries.get(id).map(|summary| summary.created_at))
            .max()
            .unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH),
    }
}

fn sort_member_rows(rows: &mut [ContainerSummary], sort: ContainerSortMode) {
    rows.sort_by(|left, right| {
        let name = || {
            left.display_name()
                .to_ascii_lowercase()
                .cmp(&right.display_name().to_ascii_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        };
        match sort {
            ContainerSortMode::NameDescending => name().reverse(),
            ContainerSortMode::NewestFirst => {
                right.created_at.cmp(&left.created_at).then_with(name)
            }
            ContainerSortMode::OldestFirst => {
                left.created_at.cmp(&right.created_at).then_with(name)
            }
            ContainerSortMode::StoppedFirst => active_rank(right.state)
                .cmp(&active_rank(left.state))
                .then_with(name),
            ContainerSortMode::RunningFirst => active_rank(left.state)
                .cmp(&active_rank(right.state))
                .then_with(name),
            _ => name(),
        }
    });
}

fn group_operation_applies(state: ContainerRuntimeState, operation: GroupOperationState) -> bool {
    match operation {
        GroupOperationState::Starting => state.is_stopped(),
        GroupOperationState::Stopping => state.is_active(),
        GroupOperationState::Restarting | GroupOperationState::Removing => true,
        GroupOperationState::Pausing => state == ContainerRuntimeState::Running,
        GroupOperationState::Unpausing => state == ContainerRuntimeState::Paused,
        GroupOperationState::Idle => false,
    }
}

fn active_rank(state: ContainerRuntimeState) -> u8 {
    match state {
        ContainerRuntimeState::Running => 0,
        ContainerRuntimeState::Restarting => 1,
        ContainerRuntimeState::Paused => 2,
        _ => 3,
    }
}

pub fn opaque_group_id(group_id: &ContainerGroupId) -> String {
    // Length prefix avoids collisions without leaking a second selection field.
    format!(
        "{}:{}{}",
        group_id.endpoint_key.len(),
        group_id.endpoint_key,
        group_id.project_name
    )
}

pub fn group_operation_name(operation: GroupOperationState) -> &'static str {
    match operation {
        GroupOperationState::Idle => "",
        GroupOperationState::Starting => "starting",
        GroupOperationState::Stopping => "stopping",
        GroupOperationState::Restarting => "restarting",
        GroupOperationState::Pausing => "pausing",
        GroupOperationState::Unpausing => "unpausing",
        GroupOperationState::Removing => "removing",
    }
}

#[derive(Debug, Clone, Copy)]
enum ErrorContext {
    List,
    Detail,
    Operation,
}

fn friendly_error(
    error: &DockerError,
    context: ErrorContext,
) -> (ContainersListState, &'static str, String) {
    match error {
        DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => (
            ContainersListState::DockerUnavailable,
            "docker_unavailable",
            "Docker Engine is unavailable. Check that Docker is running and try again.".into(),
        ),
        DockerError::PermissionDenied => (
            ContainersListState::PermissionDenied,
            "permission_denied",
            "Permission denied while accessing Docker. Check Docker socket permissions.".into(),
        ),
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => (
            ContainersListState::Error,
            "timeout",
            "The Docker container request timed out. Try again.".into(),
        ),
        DockerError::ContainerNotFound(_) => (
            ContainersListState::Error,
            "container_not_found",
            "This container no longer exists. Refresh the container list.".into(),
        ),
        DockerError::Conflict(message) => (
            ContainersListState::Error,
            "conflict",
            format!("Docker rejected the container operation: {message}"),
        ),
        DockerError::InvalidContainerConfig(message) => (
            ContainersListState::Error,
            "invalid_container",
            format!("Docker rejected the container configuration: {message}"),
        ),
        other if matches!(context, ErrorContext::Operation) => (
            ContainersListState::Error,
            "container_operation_failed",
            format!("Docker could not complete the container operation: {other}"),
        ),
        _ if matches!(context, ErrorContext::Detail) => (
            ContainersListState::Error,
            "detail_failed",
            "Container information is unavailable. Try again.".into(),
        ),
        _ => (
            ContainersListState::Error,
            "docker",
            "Could not load Docker containers. Try again.".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use tuxstack_docker_core::{
        COMPOSE_PROJECT_LABEL, COMPOSE_SERVICE_LABEL, ContainerStateDetail, EnvironmentVariable,
        MountInfo, NetworkAttachment, ResourceLimits, RestartPolicy,
    };

    use super::*;

    fn summary(
        id: &str,
        name: &str,
        state: ContainerRuntimeState,
        created: i64,
        project: Option<&str>,
        service: Option<&str>,
    ) -> ContainerSummary {
        let mut labels = BTreeMap::new();
        if let Some(project) = project {
            labels.insert(COMPOSE_PROJECT_LABEL.into(), project.into());
        }
        if let Some(service) = service {
            labels.insert(COMPOSE_SERVICE_LABEL.into(), service.into());
        }
        ContainerSummary {
            id: id.into(),
            short_id: id.chars().take(12).collect(),
            name: name.into(),
            image: format!("example/{name}:latest"),
            image_id: format!("sha256:{name}"),
            state,
            status: state.as_str().into(),
            created_at: Utc.timestamp_opt(created, 0).unwrap(),
            ports: vec![],
            labels,
        }
    }

    fn rows() -> Vec<ContainerSummary> {
        vec![
            summary(
                "a",
                "web",
                ContainerRuntimeState::Running,
                30,
                Some("demo"),
                Some("web"),
            ),
            summary(
                "b",
                "db",
                ContainerRuntimeState::Exited,
                20,
                Some("demo"),
                Some("db"),
            ),
            summary("c", "paused", ContainerRuntimeState::Paused, 10, None, None),
            summary("d", "solo", ContainerRuntimeState::Exited, 40, None, None),
            summary(
                "e",
                "restart",
                ContainerRuntimeState::Restarting,
                50,
                None,
                None,
            ),
        ]
    }

    fn ready() -> ContainersState {
        let mut state = ContainersState::new("endpoint");
        let generation = state.begin_refresh();
        assert!(state.apply_live(generation, &rows()));
        state
    }

    fn detail(summary: ContainerSummary) -> ContainerDetail {
        ContainerDetail {
            summary,
            command: vec![],
            entrypoint: vec![],
            environment: vec![EnvironmentVariable {
                name: "TOKEN".into(),
                value: Some("secret".into()),
            }],
            mounts: Vec::<MountInfo>::new(),
            networks: Vec::<NetworkAttachment>::new(),
            restart_policy: RestartPolicy {
                name: "no".into(),
                maximum_retry_count: None,
            },
            health: None,
            platform: None,
            hostname: None,
            domain_name: None,
            working_dir: None,
            user: None,
            stop_signal: None,
            stop_timeout_seconds: None,
            auto_remove: false,
            tty: false,
            open_stdin: false,
            read_only_rootfs: false,
            privileged: false,
            state_detail: ContainerStateDetail::default(),
            resource_limits: ResourceLimits::default(),
        }
    }

    #[test]
    fn initialize_is_idempotent() {
        let mut state = ContainersState::default();
        assert!(state.initialize());
        assert!(!state.initialize());
    }

    #[test]
    fn cached_then_live_uses_same_generation_and_live_wins() {
        let mut state = ContainersState::new("endpoint");
        let generation = state.begin_refresh();
        assert!(state.apply_cached(generation, &[rows()[0].clone()]));
        assert!(state.using_cache);
        assert!(state.refresh_in_progress);
        assert_eq!(state.total_count(), 1);
        assert!(state.apply_live(generation, &rows()));
        assert!(!state.using_cache);
        assert!(!state.refresh_in_progress);
        assert_eq!(state.total_count(), 5);
    }

    #[test]
    fn stale_cache_and_live_results_are_ignored() {
        let mut state = ContainersState::default();
        let stale = state.begin_refresh();
        let current = state.begin_refresh();
        assert!(!state.apply_cached(stale, &rows()));
        assert!(!state.apply_live(stale, &rows()));
        assert!(state.apply_live(current, &rows()));
    }

    #[test]
    fn all_six_list_states_are_reachable() {
        let mut state = ContainersState::default();
        assert_eq!(state.list_state, ContainersListState::Loading);
        let generation = state.begin_refresh();
        state.apply_live(generation, &rows());
        assert_eq!(state.list_state, ContainersListState::Ready);
        let generation = state.begin_refresh();
        state.apply_live(generation, &[]);
        assert_eq!(state.list_state, ContainersListState::Empty);
        let generation = state.begin_refresh();
        state.apply_list_error(generation, &DockerError::Api("x".into()));
        assert_eq!(state.list_state, ContainersListState::Error);
        let generation = state.begin_refresh();
        state.apply_list_error(generation, &DockerError::EngineUnavailable);
        assert_eq!(state.list_state, ContainersListState::DockerUnavailable);
        let generation = state.begin_refresh();
        state.apply_list_error(generation, &DockerError::PermissionDenied);
        assert_eq!(state.list_state, ContainersListState::PermissionDenied);
    }

    #[test]
    fn official_labels_create_group_and_names_never_guess() {
        let state = ready();
        assert_eq!(state.groups.len(), 1);
        assert_eq!(state.groups[0].project_name, "demo");
        assert_eq!(state.groups[0].total_count, 2);
        assert!(
            !state
                .groups
                .iter()
                .any(|group| group.project_name == "solo")
        );
    }

    #[test]
    fn visible_rows_have_all_four_sections() {
        let state = ready();
        let sections = state
            .visible_rows
            .iter()
            .filter(|row| row.row_kind == ContainerRowKind::SectionHeader)
            .map(|row| row.section)
            .collect::<Vec<_>>();
        assert_eq!(sections, ContainerSection::DISPLAY_ORDER);
    }

    #[test]
    fn expansion_survives_live_refresh() {
        let mut state = ready();
        let group_id = opaque_group_id(&state.groups[0].id);
        assert!(state.toggle_group(&group_id));
        assert!(
            state
                .visible_rows
                .iter()
                .any(|row| row.row_kind == ContainerRowKind::ContainerChild)
        );
        let generation = state.begin_refresh();
        state.apply_live(generation, &rows());
        assert!(
            state
                .visible_rows
                .iter()
                .any(|row| row.row_kind == ContainerRowKind::ContainerChild)
        );
    }

    #[test]
    fn child_search_auto_expands_group_and_only_shows_matching_child() {
        let mut state = ready();
        state.set_search("db");
        assert_eq!(
            state
                .visible_rows
                .iter()
                .filter(|row| row.row_kind == ContainerRowKind::Group)
                .count(),
            1
        );
        let children = state
            .visible_rows
            .iter()
            .filter(|row| row.row_kind == ContainerRowKind::ContainerChild)
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "db");
    }

    #[test]
    fn comprehensive_local_search_covers_image_state_service_and_labels() {
        let mut state = ready();
        for query in ["example/solo", "paused", "web", "demo"] {
            state.set_search(query);
            assert!(
                state.visible_rows.iter().any(ContainerListRow::selectable),
                "{query}"
            );
        }
    }

    #[test]
    fn all_eight_sorts_are_accepted_and_stable() {
        let mut state = ready();
        for sort in [
            ContainerSortMode::NameAscending,
            ContainerSortMode::NameDescending,
            ContainerSortMode::NewestFirst,
            ContainerSortMode::OldestFirst,
            ContainerSortMode::RunningFirst,
            ContainerSortMode::StoppedFirst,
            ContainerSortMode::ComposeGroupsFirst,
            ContainerSortMode::IndividualContainersFirst,
        ] {
            state.set_sort(sort);
            assert_eq!(state.total_count(), 5);
            assert!(!state.visible_rows.is_empty());
        }
        state.set_sort(ContainerSortMode::StoppedFirst);
        assert_eq!(state.visible_rows[0].section, ContainerSection::Stopped);
    }

    #[test]
    fn group_and_individual_sort_modes_preserve_sections() {
        let mut state = ready();
        state.set_sort(ContainerSortMode::ComposeGroupsFirst);
        let running_header = state
            .visible_rows
            .iter()
            .position(|row| row.section == ContainerSection::Running)
            .unwrap();
        assert_eq!(
            state.visible_rows[running_header + 1].row_kind,
            ContainerRowKind::Group
        );
        state.set_sort(ContainerSortMode::IndividualContainersFirst);
        assert_eq!(
            state
                .visible_rows
                .iter()
                .filter(|row| row.row_kind == ContainerRowKind::SectionHeader)
                .count(),
            4
        );
    }

    #[test]
    fn selection_is_one_enum_and_derived_values_never_conflict() {
        let mut state = ready();
        let group_id = opaque_group_id(&state.groups[0].id);
        assert!(matches!(
            state.select_row(&group_id),
            SelectionAction::GroupReady { .. }
        ));
        assert_eq!(state.selection_kind(), "group");
        assert_eq!(state.selection_id(), group_id);
        assert!(state.select_container("d") != SelectionAction::None);
        assert_eq!(state.selection_kind(), "container");
        assert_eq!(state.selection_id(), "d");
    }

    #[test]
    fn clicking_same_selection_clears_it_and_generation_advances() {
        let mut state = ready();
        state.select_row("d");
        let generation = state.selection_generation;
        assert_eq!(state.select_row("d"), SelectionAction::None);
        assert_eq!(state.selection_kind(), "none");
        assert!(state.selection_generation > generation);
    }

    #[test]
    fn selection_change_resets_detail_and_tab_state_once() {
        let mut state = ready();
        let SelectionAction::LoadContainer { generation } = state.select_row("d") else {
            panic!()
        };
        assert!(state.apply_detail(generation, &detail(rows()[3].clone())));
        state.active_tab = "logs".into();
        state.select_container("c");
        assert!(state.container_detail.is_none());
        assert_eq!(state.detail_state, ContainerDetailState::Loading);
        assert_eq!(state.active_tab, "info");
    }

    #[test]
    fn duplicate_detail_reload_reuses_the_in_flight_request() {
        let mut state = ready();
        let first = state.select_container("c");
        let SelectionAction::LoadContainer { generation } = first else {
            panic!("expected first inspect request");
        };
        assert_eq!(state.reload_detail(), SelectionAction::None);
        assert_eq!(state.selection_generation, generation);

        assert!(state.apply_detail(generation, &detail(rows()[2].clone())));
        assert!(matches!(
            state.reload_detail(),
            SelectionAction::LoadContainer { .. }
        ));
    }

    #[test]
    fn stale_detail_cannot_replace_new_selection() {
        let mut state = ready();
        let SelectionAction::LoadContainer { generation: old } = state.select_row("d") else {
            panic!()
        };
        state.select_container("c");
        assert!(!state.apply_detail(old, &detail(rows()[3].clone())));
        assert!(state.container_detail.is_none());
    }

    #[test]
    fn detail_id_must_match_current_selection() {
        let mut state = ready();
        let SelectionAction::LoadContainer { generation } = state.select_row("d") else {
            panic!()
        };
        assert!(!state.apply_detail(generation, &detail(rows()[2].clone())));
    }

    #[test]
    fn group_selection_builds_info_without_inspect() {
        let mut state = ready();
        let group_id = opaque_group_id(&state.groups[0].id);
        state.select_row(&group_id);
        assert_eq!(state.detail_state, ContainerDetailState::Ready);
        assert_eq!(state.group_detail.as_ref().unwrap().members.len(), 2);
    }

    #[test]
    fn typed_operation_rejects_duplicates_and_stale_cleanup() {
        let mut state = ready();
        let generation = state
            .begin_operation("d", ContainerOperationState::Starting)
            .unwrap();
        assert!(
            state
                .begin_operation("d", ContainerOperationState::Removing)
                .is_none()
        );
        assert!(!state.finish_operation("d", generation + 1, ContainerOperationState::Starting));
        assert!(state.finish_operation("d", generation, ContainerOperationState::Starting));
        assert!(state.operations.is_empty());
    }

    #[test]
    fn failed_operation_clears_busy_and_keeps_specific_error() {
        let mut state = ready();
        let generation = state
            .begin_operation("d", ContainerOperationState::Starting)
            .unwrap();
        assert!(state.fail_operation(
            "d",
            generation,
            ContainerOperationState::Starting,
            &DockerError::Conflict("port already allocated".into())
        ));
        assert!(state.operations.is_empty());
        assert!(state.list_error_message.contains("port already allocated"));
    }

    #[test]
    fn deleting_selected_row_selects_adjacent_row() {
        let mut state = ready();
        state.select_row("d");
        let action = state.remove_local_many(&["d".to_string()]);
        assert_ne!(action, SelectionAction::None);
        assert_ne!(state.selection_kind(), "none");
        assert!(!state.source_rows.iter().any(|row| row.id == "d"));
    }

    #[test]
    fn deleting_all_rows_clears_selection_and_operations() {
        let mut state = ready();
        state.select_row("d");
        state.begin_operation("d", ContainerOperationState::Removing);
        let ids = rows().into_iter().map(|row| row.id).collect::<Vec<_>>();
        state.remove_local_many(&ids);
        assert_eq!(state.selection_kind(), "none");
        assert!(state.operations.is_empty());
        assert_eq!(state.list_state, ContainersListState::Empty);
    }

    #[test]
    fn counts_are_container_counts_not_flat_row_counts() {
        let state = ready();
        assert_eq!(state.total_count(), 5);
        assert_eq!(state.running_count(), 1);
        assert_eq!(state.paused_count(), 1);
        assert_eq!(state.stopped_count(), 3);
        assert!(state.visible_rows.len() >= state.total_count());
    }

    #[test]
    fn live_error_preserves_nonempty_cached_rows() {
        let mut state = ContainersState::new("endpoint");
        let generation = state.begin_refresh();
        state.apply_cached(generation, &rows());
        state.apply_list_error(generation, &DockerError::EngineUnavailable);
        assert_eq!(state.list_state, ContainersListState::Ready);
        assert_eq!(state.total_count(), 5);
        assert_eq!(state.list_error_kind, "docker_unavailable");
    }

    #[test]
    fn environment_reveal_is_reset_by_selection_generation() {
        let mut state = ready();
        let SelectionAction::LoadContainer { generation } = state.select_row("d") else {
            panic!()
        };
        state.apply_detail(generation, &detail(rows()[3].clone()));
        assert!(state.reveal_environment(0));
        assert_eq!(
            state.container_detail.as_ref().unwrap().environment[0].masked_value(),
            "secret"
        );
        state.select_container("c");
        assert!(state.container_detail.is_none());
    }

    #[test]
    fn group_partial_result_reports_successes_and_failures_and_cleans_busy() {
        let mut state = ready();
        let group_id = opaque_group_id(&state.groups[0].id);
        let (generation, targets) = state
            .begin_group_operation(&group_id, GroupOperationState::Restarting)
            .unwrap();
        assert_eq!(targets.len(), 2);
        let result = GroupOperationResult {
            operation: GroupOperationState::Restarting,
            targets: vec![
                GroupTargetResult {
                    container_id: "a".into(),
                    container_name: "web".into(),
                    success: true,
                    error: String::new(),
                },
                GroupTargetResult {
                    container_id: "b".into(),
                    container_name: "db".into(),
                    success: false,
                    error: "port already in use".into(),
                },
            ],
        };
        assert_eq!(result.success_count(), 1);
        assert_eq!(result.failure_count(), 1);
        assert!(result.message().contains("db — port already in use"));
        assert!(state.finish_group_operation(&group_id, generation, result));
        assert!(state.group_operations.is_empty());
    }

    #[test]
    fn opaque_group_ids_are_collision_safe_for_endpoint_project_pairs() {
        let a = ContainerGroupId {
            endpoint_key: "ab".into(),
            project_name: "c".into(),
        };
        let b = ContainerGroupId {
            endpoint_key: "a".into(),
            project_name: "bc".into(),
        };
        assert_ne!(opaque_group_id(&a), opaque_group_id(&b));
    }
}
