//! Unified Docker Volumes QAbstractListModel/controller bridge.
//!
//! The CXX-Qt declaration lives in `resource_bridges.rs`. This module keeps
//! list, detail, dialog preparation, and operation state separate, converts
//! typed docker-core domain values into structured QVariant maps, and owns one
//! cancellation token and generation counter per asynchronous operation kind.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;
use tuxstack_client::DaemonError as DockerError;
use tuxstack_domain::{
    CloneVolumeRequest, CreateVolumeRequest, ExportVolumeRequest, PruneVolumeFilters,
    RemoveVolumeOptions, VolumeExportCompression,
};

use crate::app_state::daemon_services;
use crate::bridge::resource_bridges::qobject;
use crate::controllers::volumes::{VolumeSortMode, VolumesListState, VolumesState};
use crate::models::volume_model::{
    VolumeContainerView, VolumeDetailView, VolumeKeyValueRow, VolumePropertyRow, VolumeRow,
    format_bytes,
};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

/// Rust state backing the single QML volume model/controller.
#[derive(Default)]
pub struct VolumeListModelRust {
    pub(crate) state: VolumesState,
    pub(crate) docker_ready: bool,

    pub(crate) search_query: QString,
    pub(crate) sort_mode: QString,
    pub(crate) list_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) loading: bool,
    pub(crate) count: i32,
    pub(crate) volume_count: i32,
    pub(crate) in_use_count: i32,
    pub(crate) unused_count: i32,
    pub(crate) known_total_size_bytes: i64,
    pub(crate) known_total_size_text: QString,
    pub(crate) known_size_count: i32,
    pub(crate) unknown_size_count: i32,
    pub(crate) global_operation_in_progress: bool,
    pub(crate) operation_in_progress: bool,

    pub(crate) selected_volume_name: QString,
    pub(crate) selected_volume_busy: bool,
    pub(crate) detail_state: QString,
    pub(crate) detail_error_kind: QString,
    pub(crate) detail_error: QString,
    pub(crate) detail_name: QString,
    pub(crate) detail_driver: QString,
    pub(crate) detail_scope: QString,
    pub(crate) detail_mountpoint: QString,
    pub(crate) detail_created_text: QString,
    pub(crate) detail_size_bytes: i64,
    pub(crate) detail_size_known: bool,
    pub(crate) detail_size_text: QString,
    pub(crate) detail_ref_count_text: QString,
    pub(crate) detail_anonymous: bool,
    pub(crate) detail: QVariant,
    pub(crate) general_model: QVariantList,
    pub(crate) used_by_model: QVariantList,
    pub(crate) label_model: QVariantList,
    pub(crate) option_model: QVariantList,
    pub(crate) status_model: QVariantList,
    pub(crate) label_count: i32,
    pub(crate) option_count: i32,
    pub(crate) status_count: i32,

    pub(crate) creating: bool,
    pub(crate) create_error_message: QString,
    pub(crate) remove_preparation_active: bool,
    pub(crate) removing_volume_name: QString,
    pub(crate) remove_error_message: QString,
    pub(crate) prune_preparation_active: bool,
    pub(crate) pruning: bool,
    pub(crate) prune_candidate_model: QVariantList,
    pub(crate) prune_known_size_text: QString,
    pub(crate) prune_unknown_size_count: i32,
    pub(crate) prune_error_message: QString,
    pub(crate) exporting_volume_name: QString,
    pub(crate) export_status: QString,
    pub(crate) export_error_message: QString,
    pub(crate) cloning_source_name: QString,
    pub(crate) clone_status: QString,
    pub(crate) clone_error_message: QString,
    // docker-core deliberately rejects tar.zst until its fixed helper supports
    // it. Keep the existing QML property and advertise the actual capability.
    pub(crate) zstd_available: bool,

    pub(crate) label_search: String,
    pub(crate) label_ascending: bool,
    pub(crate) option_search: String,
    pub(crate) option_ascending: bool,
    pub(crate) status_search: String,
    pub(crate) status_ascending: bool,
    pub(crate) prune_unknown_names: HashSet<String>,
    pub(crate) prune_known_sizes: HashMap<String, u64>,

    pub(crate) refresh_cancel: Option<CancellationToken>,
    pub(crate) detail_cancel: Option<CancellationToken>,
    pub(crate) create_cancel: Option<CancellationToken>,
    pub(crate) remove_prepare_cancel: Option<CancellationToken>,
    pub(crate) remove_cancel: HashMap<String, CancellationToken>,
    pub(crate) prune_prepare_cancel: Option<CancellationToken>,
    pub(crate) prune_cancel: Option<CancellationToken>,
    pub(crate) export_cancel: Option<CancellationToken>,
    pub(crate) clone_cancel: Option<CancellationToken>,

    pub(crate) refresh_bridge_generation: u64,
    pub(crate) detail_bridge_generation: u64,
    pub(crate) create_bridge_generation: u64,
    pub(crate) remove_prepare_generation: u64,
    pub(crate) remove_bridge_generation: u64,
    pub(crate) remove_bridge_generations: HashMap<String, u64>,
    pub(crate) prune_prepare_generation: u64,
    pub(crate) prune_bridge_generation: u64,
    pub(crate) export_bridge_generation: u64,
    pub(crate) clone_bridge_generation: u64,
}

impl Drop for VolumeListModelRust {
    fn drop(&mut self) {
        cancel(&mut self.refresh_cancel);
        cancel(&mut self.detail_cancel);
        cancel(&mut self.create_cancel);
        cancel(&mut self.remove_prepare_cancel);
        cancel_map(&mut self.remove_cancel);
        cancel(&mut self.prune_prepare_cancel);
        cancel(&mut self.prune_cancel);
        cancel(&mut self.export_cancel);
        cancel(&mut self.clone_cancel);
    }
}

impl qobject::VolumeListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.state.visible_rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.state.visible_rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            257 => qv(&row.volume_name),
            258 => qv(&row.display_name),
            259 => qv(&row.driver),
            260 => qv(&row.scope),
            261 => qv(&row.mountpoint),
            262 => QVariant::from(&saturating_i64(row.size_bytes.unwrap_or_default())),
            263 => QVariant::from(&row.size_known),
            264 => qv(&row.size_text),
            265 => qv(&row
                .created_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default()),
            266 => qv(&row.created_text),
            267 => QVariant::from(&row.in_use),
            268 => QVariant::from(&saturating_i32(row.used_by_count)),
            269 => QVariant::from(&row.anonymous),
            270 => QVariant::from(&row.selected),
            271 => QVariant::from(&row.busy),
            272 => qv(&row.operation),
            273 => qv(&row.secondary_text()),
            274 => qv(&row.section),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (257, "volumeName"),
            (258, "displayName"),
            (259, "driver"),
            (260, "scope"),
            (261, "mountpoint"),
            (262, "sizeBytes"),
            (263, "sizeKnown"),
            (264, "sizeText"),
            (265, "createdAt"),
            (266, "createdText"),
            (267, "inUse"),
            (268, "usedByCount"),
            (269, "anonymous"),
            (270, "selected"),
            (271, "busy"),
            (272, "operation"),
            (273, "secondaryText"),
            (274, "section"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    pub(crate) fn initialize(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if !state.initialize() {
            tracing::debug!("VolumesController initialize ignored; already initialized");
            return;
        }
        tracing::info!("VolumesController initialized");
        self.as_mut().rust_mut().label_ascending = true;
        self.as_mut().rust_mut().option_ascending = true;
        self.as_mut().rust_mut().status_ascending = true;
        self.as_mut().apply_state(state);
        if daemon_services().is_some() {
            self.as_mut().rust_mut().docker_ready = true;
            self.as_mut().refresh();
        } else {
            tracing::debug!("VolumesController waiting for Docker connection");
        }
    }

    pub(crate) fn refresh(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().refresh_cancel);
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        bump(&mut self.as_mut().rust_mut().detail_bridge_generation);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().refresh_bridge_generation);
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.begin_refresh();
            self.as_mut().apply_state(state);
            generation
        };
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_list_error(state_generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        tracing::info!("Loading Docker volumes");
        let token = CancellationToken::new();
        self.as_mut().rust_mut().refresh_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            // Stage A: the base list only — no container association, no
            // /system/df — so the names appear as fast as possible. Usage is
            // patched in Stage B without blocking this first paint.
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.volumes.list_volume_summaries() => result,
            };
            let base_names: Vec<String> = match &result {
                Ok(volumes) => volumes.iter().map(|v| v.name.clone()).collect(),
                Err(_) => Vec::new(),
            };
            let base_result = result;
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.refresh_bridge_generation {
                        return;
                    }
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match base_result {
                        Ok(volumes) => {
                            tracing::info!(count = volumes.len(), "Docker returned volumes");
                            state.apply_list(state_generation, &volumes)
                        }
                        Err(error) => {
                            tracing::debug!(%error, "Docker volume list request failed");
                            state.apply_list_error(state_generation, &error)
                        }
                    };
                    if !applied {
                        return;
                    }
                    model.as_mut().rust_mut().refresh_cancel = None;
                    let selected = state.selected_volume_name.clone();
                    tracing::info!(
                        visible_count = state.visible_rows.len(),
                        has_selection = !selected.is_empty(),
                        "Updating volume model"
                    );
                    model.as_mut().apply_state(state);
                    if !selected.is_empty() {
                        tracing::info!("Selecting first or preserved volume");
                        model.as_mut().load_detail(&selected, false);
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume refresh result dropped"));

            // Stage B: associate containers and system-df usage in the
            // background and patch rows in place (no full reset).
            if base_names.is_empty() {
                return;
            }
            let (references, usage) = services.volumes.enrich_usage(&base_names).await;
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.refresh_bridge_generation {
                        return;
                    }
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let patched = state.patch_usage(&references, &usage);
                    if patched == 0 {
                        return;
                    }
                    tracing::info!(patched_rows = patched, "Patched volume usage");
                    model.as_mut().apply_state(state);
                })
                .ok();
        });
    }

    pub(crate) fn update_search_query(mut self: Pin<&mut Self>, query: &QString) {
        let mut state = self.as_mut().rust_mut().state.clone();
        let detail_generation = state.set_search_query(&query.to_string());
        let selected = state.selected_volume_name.clone();
        self.as_mut().apply_state(state);
        if let Some(detail_generation) = detail_generation {
            cancel(&mut self.as_mut().rust_mut().detail_cancel);
            if !selected.is_empty() {
                self.as_mut()
                    .start_detail_request(selected, detail_generation);
            }
        }
    }

    pub(crate) fn update_sort_mode(mut self: Pin<&mut Self>, mode: &QString) {
        let Some(mode) = sort_mode_from_name(&mode.to_string()) else {
            return;
        };
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_sort_mode(mode);
        self.as_mut().apply_state(state);
    }

    pub(crate) fn select_volume(mut self: Pin<&mut Self>, volume_name: &QString) {
        self.as_mut().load_detail(&volume_name.to_string(), true);
    }

    pub(crate) fn reload_selected_volume(mut self: Pin<&mut Self>) {
        let name = self.state.selected_volume_name.clone();
        if !name.is_empty() {
            self.as_mut().load_detail(&name, false);
        }
    }

    fn load_detail(mut self: Pin<&mut Self>, name: &str, select: bool) {
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = if select {
                state.select(name)
            } else {
                state.begin_selected_inspect()
            };
            let Some(generation) = generation else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        self.as_mut()
            .start_detail_request(name.to_string(), generation);
    }

    fn start_detail_request(mut self: Pin<&mut Self>, name: String, state_generation: u64) {
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().detail_bridge_generation);
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_detail_error(state_generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().detail_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.volumes.inspect_volume(&name) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.detail_bridge_generation {
                        return;
                    }
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match result {
                        Ok(detail) => state.apply_detail(state_generation, &detail),
                        Err(error) => {
                            tracing::debug!(%error, volume_name = %name, "volume detail request failed");
                            state.apply_detail_error(state_generation, &error)
                        }
                    };
                    if applied {
                        model.as_mut().rust_mut().detail_cancel = None;
                        if state.detail_state
                            == crate::controllers::volumes::VolumeDetailState::Ready
                        {
                            tracing::info!("Volume detail loaded");
                        }
                        model.as_mut().apply_state(state);
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume detail result dropped"));
        });
    }

    pub(crate) fn set_connection_state(
        mut self: Pin<&mut Self>,
        docker_status: i32,
        _message: &QString,
    ) {
        if !self.state.initialized {
            return;
        }
        if docker_status == 1 {
            if !self.as_mut().rust_mut().docker_ready {
                self.as_mut().rust_mut().docker_ready = true;
                self.as_mut().refresh();
            }
            return;
        }
        self.as_mut().rust_mut().docker_ready = false;
        self.as_mut().cancel_all_tokens();
        let initialized = self.state.initialized;
        let sort_mode = self.state.sort_mode;
        let search_query = self.state.search_query.clone();
        let mut state = VolumesState {
            initialized,
            sort_mode,
            search_query,
            list_state: if docker_status == 0 {
                VolumesListState::Loading
            } else if docker_status == 2 {
                VolumesListState::DockerUnavailable
            } else if docker_status == 3 {
                VolumesListState::PermissionDenied
            } else {
                VolumesListState::Error
            },
            ..Default::default()
        };
        state.list_error_kind = match docker_status {
            2 => "docker_unavailable",
            3 => "permission_denied",
            4 => "docker",
            _ => "",
        }
        .into();
        state.list_error_message = safe_connection_message(docker_status).into();
        self.as_mut().clear_dialog_state();
        self.as_mut().apply_state(state);
    }

    pub(crate) fn create_volume(
        mut self: Pin<&mut Self>,
        name: &QString,
        driver: &QString,
        driver_options: &QVariantList,
        labels: &QVariantList,
    ) {
        self.as_mut().set_create_error_message(QString::default());
        let request = match create_request(
            &name.to_string(),
            &driver.to_string(),
            driver_options,
            labels,
        ) {
            Ok(request) => request,
            Err(message) => {
                self.as_mut()
                    .set_create_error_message(QString::from(message));
                return;
            }
        };
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_create() else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        cancel(&mut self.as_mut().rust_mut().create_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().create_bridge_generation);
        let Some(services) = daemon_services() else {
            self.as_mut().finish_create_error(
                bridge_generation,
                state_generation,
                DockerError::EngineUnavailable,
            );
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().create_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let operation = services.volumes.create_volume(request);
            tokio::pin!(operation);
            let result = tokio::select! {
                biased;
                _ = token.cancelled() => {
                    // A Docker create request cannot be aborted safely once the
                    // daemon has accepted it. Wait for acknowledgement and
                    // remove any newly-created volume before reporting cancel.
                    match operation.await {
                        Ok(detail) => {
                            let created_name = detail.summary.name;
                            match services.volumes.remove_volume(
                                &created_name,
                                RemoveVolumeOptions { force: true },
                            ).await {
                                Ok(()) | Err(DockerError::VolumeNotFound(_)) => {
                                    Err(DockerError::OperationCancelled)
                                }
                                Err(cleanup) => Err(DockerError::CleanupFailed(format!(
                                    "cancelled volume creation cleanup failed: {cleanup}"
                                ))),
                            }
                        }
                        Err(_) => Err(DockerError::OperationCancelled),
                    }
                },
                result = &mut operation => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.create_bridge_generation {
                        return;
                    }
                    model.as_mut().rust_mut().create_cancel = None;
                    let mut state = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(detail)
                            if state.finish_create(state_generation, &detail.summary.name) =>
                        {
                            let name = detail.summary.name;
                            model.as_mut().apply_state(state);
                            model.as_mut().volume_created(QString::from(&name));
                            model.as_mut().refresh();
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::debug!(%error, "volume creation failed");
                            if state.fail_global_operation(state_generation, &error) {
                                let cancelled = matches!(error, DockerError::OperationCancelled);
                                let message = state.operation_error_message.clone();
                                model.as_mut().apply_state(state);
                                if !cancelled {
                                    model
                                        .as_mut()
                                        .set_create_error_message(QString::from(message));
                                }
                            }
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume create result dropped"));
        });
    }

    fn finish_create_error(
        mut self: Pin<&mut Self>,
        bridge_generation: u64,
        state_generation: u64,
        error: DockerError,
    ) {
        if bridge_generation != self.create_bridge_generation {
            return;
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.fail_global_operation(state_generation, &error) {
            let message = state.operation_error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut()
                .set_create_error_message(QString::from(message));
        }
    }

    pub(crate) fn cancel_create(mut self: Pin<&mut Self>) {
        if let Some(token) = self.as_mut().rust_mut().create_cancel.as_ref() {
            token.cancel();
        }
    }

    pub(crate) fn prepare_remove_volume(mut self: Pin<&mut Self>, volume_name: &QString) {
        cancel(&mut self.as_mut().rust_mut().remove_prepare_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().remove_prepare_generation);
        self.as_mut().set_remove_preparation_active(false);
        let name = volume_name.to_string();
        if name.trim().is_empty() {
            return;
        }
        let Some(services) = daemon_services() else {
            self.as_mut()
                .remove_preparation_failed(QString::from("Docker Engine is not available."));
            return;
        };
        self.as_mut().set_remove_preparation_active(true);
        self.as_mut().set_remove_error_message(QString::default());
        let token = CancellationToken::new();
        self.as_mut().rust_mut().remove_prepare_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.volumes.inspect_volume(&name) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.remove_prepare_generation {
                        return;
                    }
                    model.as_mut().rust_mut().remove_prepare_cancel = None;
                    model.as_mut().set_remove_preparation_active(false);
                    match result {
                        Ok(detail) => {
                            let row = VolumeRow::from(&detail.summary);
                            model.as_mut().remove_prepared(
                                QString::from(&row.volume_name),
                                QString::from(&row.driver),
                                QString::from(&row.size_text),
                                saturating_i32(row.used_by_count),
                                QString::from(&row.mountpoint),
                            );
                        }
                        Err(error) => {
                            tracing::debug!(%error, volume_name = %name, "volume remove preparation failed");
                            model.as_mut().remove_preparation_failed(QString::from(
                                safe_operation_error(&error),
                            ));
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume remove preparation result dropped"));
        });
    }

    pub(crate) fn remove_volume(mut self: Pin<&mut Self>, volume_name: &QString, force: bool) {
        let name = volume_name.to_string();
        self.as_mut().set_remove_error_message(QString::default());
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_remove(&name) else {
                self.as_mut().set_remove_error_message(QString::from(
                    "This volume no longer exists. Refresh the volume list.",
                ));
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let bridge_generation = bump(&mut self.as_mut().rust_mut().remove_bridge_generation);
        self.as_mut()
            .rust_mut()
            .remove_bridge_generations
            .insert(name.clone(), bridge_generation);
        let Some(services) = daemon_services() else {
            self.as_mut().finish_remove_error(
                bridge_generation,
                state_generation,
                &name,
                DockerError::EngineUnavailable,
            );
            return;
        };
        let token = CancellationToken::new();
        self.as_mut()
            .rust_mut()
            .remove_cancel
            .insert(name.clone(), token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => Err(DockerError::OperationCancelled),
                result = services.volumes.remove_volume(&name, RemoveVolumeOptions { force }) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if model.remove_bridge_generations.get(&name).copied()
                        != Some(bridge_generation)
                    {
                        return;
                    }
                    model.as_mut().rust_mut().remove_cancel.remove(&name);
                    model
                        .as_mut()
                        .rust_mut()
                        .remove_bridge_generations
                        .remove(&name);
                    let mut state = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(()) if state.finish_remove(state_generation, &name) => {
                            state.remove_local(&name);
                            model.as_mut().apply_state(state);
                            model.as_mut().volume_removed(QString::from(&name));
                            model.as_mut().refresh();
                        }
                        Ok(()) => {}
                        Err(error) => {
                            tracing::debug!(%error, volume_name = %name, "volume removal failed");
                            if state.fail_volume_operation(
                                state_generation,
                                &name,
                                crate::controllers::volumes::VolumeOperation::Removing,
                                &error,
                            ) {
                                let message = state.operation_error_message.clone();
                                model.as_mut().apply_state(state);
                                model
                                    .as_mut()
                                    .set_remove_error_message(QString::from(message));
                            }
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume remove result dropped"));
        });
    }

    fn finish_remove_error(
        mut self: Pin<&mut Self>,
        bridge_generation: u64,
        state_generation: u64,
        name: &str,
        error: DockerError,
    ) {
        if self.remove_bridge_generations.get(name).copied() != Some(bridge_generation) {
            return;
        }
        self.as_mut()
            .rust_mut()
            .remove_bridge_generations
            .remove(name);
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.fail_volume_operation(
            state_generation,
            name,
            crate::controllers::volumes::VolumeOperation::Removing,
            &error,
        ) {
            let message = state.operation_error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut()
                .set_remove_error_message(QString::from(message));
        }
    }

    pub(crate) fn prepare_prune_volumes(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().prune_prepare_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().prune_prepare_generation);
        self.as_mut().set_prune_preparation_active(false);
        let Some(services) = daemon_services() else {
            self.as_mut()
                .prune_preparation_failed(QString::from("Docker Engine is not available."));
            return;
        };
        self.as_mut().set_prune_preparation_active(true);
        self.as_mut().set_prune_error_message(QString::default());
        let token = CancellationToken::new();
        self.as_mut().rust_mut().prune_prepare_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.volumes.list_all_volumes() => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.prune_prepare_generation {
                        return;
                    }
                    model.as_mut().rust_mut().prune_prepare_cancel = None;
                    model.as_mut().set_prune_preparation_active(false);
                    match result {
                        Ok(volumes) => {
                            let candidates: Vec<_> = volumes
                                .iter()
                                .filter(|volume| !volume.in_use())
                                .map(VolumeRow::from)
                                .collect();
                            model.as_mut().set_prune_candidates(&candidates);
                            model.as_mut().prune_prepared();
                        }
                        Err(error) => {
                            tracing::debug!(%error, "volume prune preparation failed");
                            model.as_mut().prune_preparation_failed(QString::from(
                                safe_operation_error(&error),
                            ));
                        }
                    }
                })
                .unwrap_or_else(
                    |error| tracing::debug!(%error, "volume prune preparation result dropped"),
                );
        });
    }

    fn set_prune_candidates(mut self: Pin<&mut Self>, candidates: &[VolumeRow]) {
        let known_total = candidates.iter().fold(0_u64, |total, row| {
            total.saturating_add(row.size_bytes.unwrap_or_default())
        });
        let unknown_names: HashSet<_> = candidates
            .iter()
            .filter(|row| !row.size_known)
            .map(|row| row.volume_name.clone())
            .collect();
        let known_sizes: HashMap<_, _> = candidates
            .iter()
            .filter_map(|row| row.size_bytes.map(|size| (row.volume_name.clone(), size)))
            .collect();
        self.as_mut()
            .set_prune_candidate_model(prune_candidate_rows(candidates));
        self.as_mut()
            .set_prune_known_size_text(QString::from(format_bytes(known_total)));
        self.as_mut()
            .set_prune_unknown_size_count(saturating_i32(unknown_names.len()));
        self.as_mut().rust_mut().prune_unknown_names = unknown_names;
        self.as_mut().rust_mut().prune_known_sizes = known_sizes;
    }

    pub(crate) fn prune_volumes(mut self: Pin<&mut Self>) {
        self.as_mut().set_prune_error_message(QString::default());
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_prune() else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        cancel(&mut self.as_mut().rust_mut().prune_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().prune_bridge_generation);
        let Some(services) = daemon_services() else {
            self.as_mut().finish_prune_error(
                bridge_generation,
                state_generation,
                DockerError::EngineUnavailable,
            );
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().prune_cancel = Some(token.clone());
        let filters = PruneVolumeFilters {
            filters: BTreeMap::from([("all".into(), vec!["true".into()])]),
        };
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = if token.is_cancelled() {
                Err(DockerError::OperationCancelled)
            } else {
                let operation = services.volumes.prune_volumes(filters);
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => result,
                    // The daemon prune itself is not reversible. If cancellation
                    // arrives after submission, await its real result rather
                    // than falsely claiming that deleted data was preserved.
                    _ = token.cancelled() => operation.await,
                }
            };
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.prune_bridge_generation {
                        return;
                    }
                    model.as_mut().rust_mut().prune_cancel = None;
                    let mut state = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(result) if state.finish_prune(state_generation) => {
                            let unknown = result
                                .volumes_deleted
                                .iter()
                                .filter(|name| {
                                    model.prune_unknown_names.contains(*name)
                                        || !model.prune_known_sizes.contains_key(*name)
                                })
                                .count();
                            let prepared_known_reclaimed = result
                                .volumes_deleted
                                .iter()
                                .filter_map(|name| model.prune_known_sizes.get(name))
                                .fold(0_u64, |total, size| total.saturating_add(*size));
                            let reclaimed = format_bytes(
                                result
                                    .space_reclaimed_bytes
                                    .unwrap_or(prepared_known_reclaimed),
                            );
                            model.as_mut().apply_state(state);
                            model.as_mut().volumes_pruned(
                                saturating_i32(result.volumes_deleted.len()),
                                QString::from(reclaimed),
                                saturating_i32(unknown),
                            );
                            model.as_mut().refresh();
                        }
                        Ok(_) => {}
                        Err(DockerError::OperationCancelled) => {
                            if state.finish_prune(state_generation) {
                                model.as_mut().apply_state(state);
                            }
                        }
                        Err(error) => {
                            tracing::debug!(%error, "volume prune failed");
                            if state.fail_global_operation(state_generation, &error) {
                                let message = state.operation_error_message.clone();
                                model.as_mut().apply_state(state);
                                model
                                    .as_mut()
                                    .set_prune_error_message(QString::from(message));
                            }
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume prune result dropped"));
        });
    }

    fn finish_prune_error(
        mut self: Pin<&mut Self>,
        bridge_generation: u64,
        state_generation: u64,
        error: DockerError,
    ) {
        if bridge_generation != self.prune_bridge_generation {
            return;
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.fail_global_operation(state_generation, &error) {
            let message = state.operation_error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut()
                .set_prune_error_message(QString::from(message));
        }
    }

    pub(crate) fn cancel_prune(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.request_cancel_prune() {
            self.as_mut().apply_state(state);
            if let Some(token) = self.as_mut().rust_mut().prune_cancel.as_ref() {
                token.cancel();
            }
        }
    }

    pub(crate) fn export_volume(
        mut self: Pin<&mut Self>,
        volume_name: &QString,
        destination: &QString,
        format: &QString,
    ) {
        self.as_mut().set_export_error_message(QString::default());
        let name = volume_name.to_string();
        let destination = match local_destination(&destination.to_string()) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut()
                    .set_export_error_message(QString::from(message));
                return;
            }
        };
        let compression = match format.to_string().as_str() {
            "tar" => VolumeExportCompression::Tar,
            "tar_gzip" => VolumeExportCompression::TarGzip,
            "tar_zstd" => {
                self.as_mut().set_export_error_message(QString::from(
                    "Zstandard volume export is not available.",
                ));
                return;
            }
            _ => {
                self.as_mut()
                    .set_export_error_message(QString::from("Unknown volume export format."));
                return;
            }
        };
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_export(&name) else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        self.as_mut()
            .set_export_status(QString::from("Exporting volume…"));
        cancel(&mut self.as_mut().rust_mut().export_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().export_bridge_generation);
        let Some(services) = daemon_services() else {
            self.as_mut().finish_export_error(
                bridge_generation,
                state_generation,
                DockerError::EngineUnavailable,
            );
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().export_cancel = Some(token.clone());
        let destination_text = destination.display().to_string();
        let request = ExportVolumeRequest {
            volume_name: name.clone(),
            destination,
            compression,
        };
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.volumes.export_volume(request, token).await;
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.export_bridge_generation {
                        return;
                    }
                    model.as_mut().rust_mut().export_cancel = None;
                    model.as_mut().set_export_status(QString::default());
                    let mut state = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(()) if state.finish_export(state_generation) => {
                            model.as_mut().apply_state(state);
                            model.as_mut().export_completed(
                                QString::from(&name),
                                QString::from(&destination_text),
                            );
                        }
                        Ok(()) => {}
                        Err(DockerError::OperationCancelled) => {
                            if state.finish_export(state_generation) {
                                model.as_mut().apply_state(state);
                            }
                        }
                        Err(error) => {
                            tracing::debug!(%error, volume_name = %name, "volume export failed");
                            if state.fail_export(state_generation, &error) {
                                let message = state.operation_error_message.clone();
                                model.as_mut().apply_state(state);
                                model
                                    .as_mut()
                                    .set_export_error_message(QString::from(message));
                            }
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume export result dropped"));
        });
    }

    fn finish_export_error(
        mut self: Pin<&mut Self>,
        bridge_generation: u64,
        state_generation: u64,
        error: DockerError,
    ) {
        if bridge_generation != self.export_bridge_generation {
            return;
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.fail_export(state_generation, &error) {
            let message = state.operation_error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut().set_export_status(QString::default());
            self.as_mut()
                .set_export_error_message(QString::from(message));
        }
    }

    pub(crate) fn cancel_export(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.request_cancel_export() {
            self.as_mut().apply_state(state);
            if let Some(token) = self.as_mut().rust_mut().export_cancel.as_ref() {
                token.cancel();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn clone_volume(
        mut self: Pin<&mut Self>,
        source_volume: &QString,
        target_name: &QString,
        target_driver: &QString,
        target_driver_options: &QVariantList,
        copy_labels: bool,
        _cleanup_failed: bool,
    ) {
        // docker-core always cleans an incomplete target. The legacy QML flag
        // remains in the invokable contract, but weakening cleanup is not
        // supported because it would leave partially copied data behind.
        self.as_mut().set_clone_error_message(QString::default());
        let options = match parse_key_value_entries(target_driver_options, "driver option") {
            Ok(options) => options,
            Err(message) => {
                self.as_mut()
                    .set_clone_error_message(QString::from(message));
                return;
            }
        };
        let source = source_volume.to_string().trim().to_string();
        let target = target_name.to_string().trim().to_string();
        if source.is_empty() || target.is_empty() {
            self.as_mut().set_clone_error_message(QString::from(
                "Source and target volume names are required.",
            ));
            return;
        }
        let state_generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_clone(&source) else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        self.as_mut()
            .set_clone_status(QString::from("Cloning volume…"));
        cancel(&mut self.as_mut().rust_mut().clone_cancel);
        let bridge_generation = bump(&mut self.as_mut().rust_mut().clone_bridge_generation);
        let Some(services) = daemon_services() else {
            self.as_mut().finish_clone_error(
                bridge_generation,
                state_generation,
                DockerError::EngineUnavailable,
            );
            return;
        };
        let request = CloneVolumeRequest {
            source_volume: source.clone(),
            target_name: target.clone(),
            target_driver: optional(&target_driver.to_string()),
            target_driver_options: options,
            copy_labels,
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().clone_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.volumes.clone_volume(request, token).await;
            qt_thread
                .queue(move |mut model| {
                    if bridge_generation != model.clone_bridge_generation {
                        return;
                    }
                    model.as_mut().rust_mut().clone_cancel = None;
                    model.as_mut().set_clone_status(QString::default());
                    let mut state = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(detail)
                            if state.finish_clone(state_generation, Some(&detail.summary.name)) =>
                        {
                            let created = detail.summary.name;
                            model.as_mut().apply_state(state);
                            model
                                .as_mut()
                                .clone_completed(QString::from(&source), QString::from(&created));
                            model.as_mut().refresh();
                        }
                        Ok(_) => {}
                        Err(DockerError::OperationCancelled) => {
                            if state.finish_clone(state_generation, None) {
                                model.as_mut().apply_state(state);
                            }
                        }
                        Err(error) => {
                            tracing::debug!(%error, volume_name = %source, "volume clone failed");
                            if state.fail_clone(state_generation, &error) {
                                let message = state.operation_error_message.clone();
                                model.as_mut().apply_state(state);
                                model
                                    .as_mut()
                                    .set_clone_error_message(QString::from(message));
                            }
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "volume clone result dropped"));
        });
    }

    fn finish_clone_error(
        mut self: Pin<&mut Self>,
        bridge_generation: u64,
        state_generation: u64,
        error: DockerError,
    ) {
        if bridge_generation != self.clone_bridge_generation {
            return;
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.fail_clone(state_generation, &error) {
            let message = state.operation_error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut().set_clone_status(QString::default());
            self.as_mut()
                .set_clone_error_message(QString::from(message));
        }
    }

    pub(crate) fn cancel_clone(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.request_cancel_clone() {
            self.as_mut().apply_state(state);
            if let Some(token) = self.as_mut().rust_mut().clone_cancel.as_ref() {
                token.cancel();
            }
        }
    }

    pub(crate) fn navigate_to_container(mut self: Pin<&mut Self>, container_id: &QString) {
        self.as_mut()
            .container_navigation_requested(container_id.clone());
    }

    pub(crate) fn set_label_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().label_search = normalized_query(query);
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn set_label_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().label_ascending = ascending;
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn set_option_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().option_search = normalized_query(query);
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn set_option_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().option_ascending = ascending;
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn set_status_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().status_search = normalized_query(query);
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn set_status_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().status_ascending = ascending;
        self.as_mut().sync_filtered_detail_tables();
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_all_tokens();
        let initialized = self.state.initialized;
        self.as_mut().clear_dialog_state();
        self.as_mut().apply_state(VolumesState {
            initialized,
            ..Default::default()
        });
    }

    fn cancel_all_tokens(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().refresh_cancel);
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        cancel(&mut self.as_mut().rust_mut().create_cancel);
        cancel(&mut self.as_mut().rust_mut().remove_prepare_cancel);
        cancel_map(&mut self.as_mut().rust_mut().remove_cancel);
        self.as_mut().rust_mut().remove_bridge_generations.clear();
        cancel(&mut self.as_mut().rust_mut().prune_prepare_cancel);
        cancel(&mut self.as_mut().rust_mut().prune_cancel);
        cancel(&mut self.as_mut().rust_mut().export_cancel);
        cancel(&mut self.as_mut().rust_mut().clone_cancel);
        bump(&mut self.as_mut().rust_mut().refresh_bridge_generation);
        bump(&mut self.as_mut().rust_mut().detail_bridge_generation);
        bump(&mut self.as_mut().rust_mut().create_bridge_generation);
        bump(&mut self.as_mut().rust_mut().remove_prepare_generation);
        bump(&mut self.as_mut().rust_mut().remove_bridge_generation);
        bump(&mut self.as_mut().rust_mut().prune_prepare_generation);
        bump(&mut self.as_mut().rust_mut().prune_bridge_generation);
        bump(&mut self.as_mut().rust_mut().export_bridge_generation);
        bump(&mut self.as_mut().rust_mut().clone_bridge_generation);
    }

    fn clear_dialog_state(mut self: Pin<&mut Self>) {
        self.as_mut().set_create_error_message(QString::default());
        self.as_mut().set_remove_preparation_active(false);
        self.as_mut().set_remove_error_message(QString::default());
        self.as_mut().set_prune_preparation_active(false);
        self.as_mut().set_prune_error_message(QString::default());
        self.as_mut().set_export_status(QString::default());
        self.as_mut().set_export_error_message(QString::default());
        self.as_mut().set_clone_status(QString::default());
        self.as_mut().set_clone_error_message(QString::default());
    }

    fn apply_state(mut self: Pin<&mut Self>, state: VolumesState) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().state = state;
        self.as_mut().end_reset_model();
        let state = self.state.clone();
        let size = state.size_summary();
        let search_query = QString::from(&state.search_query);
        if self.search_query != search_query {
            self.as_mut().rust_mut().search_query = search_query;
            self.as_mut().search_query_changed();
        }
        let sort_mode = QString::from(sort_mode_name(state.sort_mode));
        if self.sort_mode != sort_mode {
            self.as_mut().rust_mut().sort_mode = sort_mode;
            self.as_mut().sort_mode_changed();
        }
        self.as_mut()
            .set_list_state(QString::from(state.list_state.as_str()));
        self.as_mut()
            .set_error_kind(QString::from(&state.list_error_kind));
        self.as_mut()
            .set_error_message(QString::from(&state.list_error_message));
        self.as_mut()
            .set_loading(state.list_state == VolumesListState::Loading);
        self.as_mut()
            .set_count(saturating_i32(state.visible_rows.len()));
        self.as_mut()
            .set_volume_count(saturating_i32(state.volume_count()));
        self.as_mut()
            .set_in_use_count(saturating_i32(state.in_use_count()));
        self.as_mut()
            .set_unused_count(saturating_i32(state.unused_count()));
        self.as_mut()
            .set_known_total_size_bytes(saturating_i64(size.known_total_bytes));
        self.as_mut()
            .set_known_total_size_text(QString::from(state.known_total_size_text()));
        self.as_mut()
            .set_known_size_count(saturating_i32(size.known_count));
        self.as_mut()
            .set_unknown_size_count(saturating_i32(size.unknown_count));
        self.as_mut()
            .set_global_operation_in_progress(state.global_operation.is_some());
        self.as_mut()
            .set_operation_in_progress(state.operation_in_progress());
        self.as_mut()
            .set_selected_volume_name(QString::from(&state.selected_volume_name));
        self.as_mut()
            .set_selected_volume_busy(state.operations.contains_key(&state.selected_volume_name));
        self.as_mut()
            .set_detail_state(QString::from(state.detail_state.as_str()));
        self.as_mut()
            .set_detail_error_kind(QString::from(&state.detail_error_kind));
        self.as_mut()
            .set_detail_error(QString::from(&state.detail_error_message));
        self.as_mut().set_creating(state.creating());
        self.as_mut().set_pruning(state.pruning());
        self.as_mut()
            .set_removing_volume_name(QString::from(operation_name(&state, "removing")));
        self.as_mut()
            .set_exporting_volume_name(QString::from(if state.export_task.active {
                state.export_task.volume_name.as_str()
            } else {
                ""
            }));
        self.as_mut()
            .set_cloning_source_name(QString::from(if state.clone_task.active {
                state.clone_task.volume_name.as_str()
            } else {
                ""
            }));
        self.as_mut().sync_detail(state.detail.as_ref());
    }

    fn sync_detail(mut self: Pin<&mut Self>, detail: Option<&VolumeDetailView>) {
        let empty = VolumeDetailView::default();
        let detail = detail.unwrap_or(&empty);
        self.as_mut()
            .set_detail_name(QString::from(&detail.volume_name));
        self.as_mut()
            .set_detail_driver(QString::from(&detail.driver));
        self.as_mut().set_detail_scope(QString::from(&detail.scope));
        self.as_mut()
            .set_detail_mountpoint(QString::from(&detail.mountpoint));
        self.as_mut()
            .set_detail_created_text(QString::from(&detail.created_text));
        self.as_mut()
            .set_detail_size_bytes(saturating_i64(detail.size_bytes.unwrap_or_default()));
        self.as_mut().set_detail_size_known(detail.size_known);
        self.as_mut()
            .set_detail_size_text(QString::from(&detail.size_text));
        self.as_mut()
            .set_detail_ref_count_text(QString::from(&detail.ref_count_text));
        self.as_mut().set_detail_anonymous(detail.anonymous);
        self.as_mut().set_detail(detail_variant(detail));
        self.as_mut()
            .set_general_model(property_rows(&detail.general));
        self.as_mut()
            .set_used_by_model(container_rows(&detail.used_by));
        self.as_mut()
            .set_label_count(saturating_i32(detail.labels.len()));
        self.as_mut()
            .set_option_count(saturating_i32(detail.options.len()));
        self.as_mut()
            .set_status_count(saturating_i32(detail.status.len()));
        self.as_mut().sync_filtered_detail_tables();
    }

    fn sync_filtered_detail_tables(mut self: Pin<&mut Self>) {
        let Some(detail) = self.state.detail.clone() else {
            self.as_mut().set_label_model(QVariantList::default());
            self.as_mut().set_option_model(QVariantList::default());
            self.as_mut().set_status_model(QVariantList::default());
            return;
        };
        let label_model = key_value_rows(&filtered_pairs(
            &detail.labels,
            &self.label_search,
            self.label_ascending,
        ));
        let option_model = key_value_rows(&filtered_pairs(
            &detail.options,
            &self.option_search,
            self.option_ascending,
        ));
        let status_model = key_value_rows(&filtered_pairs(
            &detail.status,
            &self.status_search,
            self.status_ascending,
        ));
        self.as_mut().set_label_model(label_model);
        self.as_mut().set_option_model(option_model);
        self.as_mut().set_status_model(status_model);
    }
}

fn create_request(
    name: &str,
    driver: &str,
    driver_options: &QVariantList,
    labels: &QVariantList,
) -> Result<CreateVolumeRequest, String> {
    let driver = driver.trim();
    if driver.is_empty() {
        return Err("A volume driver is required.".into());
    }
    Ok(CreateVolumeRequest {
        name: optional(name),
        driver: Some(driver.into()),
        driver_options: parse_key_value_entries(driver_options, "driver option")?,
        labels: parse_key_value_entries(labels, "label")?,
    })
}

/// Safely decode QML's QVariantList of `{ key, value }` objects. Every entry
/// must really be a map with string values; malformed, empty, and duplicate
/// keys are rejected without formatting/logging any submitted values.
fn parse_key_value_entries(
    entries: &QVariantList,
    kind: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for entry in entries.iter() {
        let Some(map) = entry.value::<QVariantMap>() else {
            return Err(format!("A {kind} entry is malformed."));
        };
        let Some(key_variant) = map.get(&QString::from("key")) else {
            return Err(format!("A {kind} entry has no key."));
        };
        let Some(value_variant) = map.get(&QString::from("value")) else {
            return Err(format!("A {kind} entry has no value."));
        };
        let Some(key) = key_variant.value::<QString>() else {
            return Err(format!("A {kind} key is not text."));
        };
        let Some(value) = value_variant.value::<QString>() else {
            return Err(format!("A {kind} value is not text."));
        };
        let key = key.to_string().trim().to_string();
        if key.is_empty() {
            return Err(format!("A {kind} key is empty."));
        }
        if result.insert(key, value.to_string()).is_some() {
            return Err(format!("A duplicate {kind} key was provided."));
        }
    }
    Ok(result)
}

fn detail_variant(detail: &VolumeDetailView) -> QVariant {
    if detail.volume_name.is_empty() {
        return QVariant::default();
    }
    let mut map = QVariantMap::default();
    for (key, value) in [
        ("volumeName", detail.volume_name.as_str()),
        ("displayName", detail.display_name.as_str()),
        ("driver", detail.driver.as_str()),
        ("scope", detail.scope.as_str()),
        ("mountpoint", detail.mountpoint.as_str()),
        ("createdText", detail.created_text.as_str()),
        ("sizeText", detail.size_text.as_str()),
        ("refCountText", detail.ref_count_text.as_str()),
        ("anonymousText", detail.anonymous_text.as_str()),
    ] {
        insert(&mut map, key, value);
    }
    map.insert(
        QString::from("sizeBytes"),
        QVariant::from(&saturating_i64(detail.size_bytes.unwrap_or_default())),
    );
    map.insert(
        QString::from("sizeKnown"),
        QVariant::from(&detail.size_known),
    );
    map.insert(
        QString::from("anonymous"),
        QVariant::from(&detail.anonymous),
    );
    QVariant::from(&map)
}

fn property_rows(rows: &[VolumePropertyRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "label", &row.label);
            insert(&mut map, "value", &row.value);
            map.insert(QString::from("copyable"), QVariant::from(&row.copyable));
            QVariant::from(&map)
        })
        .collect()
}

fn key_value_rows(rows: &[VolumeKeyValueRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "key", &row.key);
            insert(&mut map, "value", &row.value);
            QVariant::from(&map)
        })
        .collect()
}

fn container_rows(rows: &[VolumeContainerView]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            for (key, value) in [
                ("containerId", row.container_id.as_str()),
                ("shortId", row.short_id.as_str()),
                ("name", row.name.as_str()),
                ("state", row.state.as_str()),
                ("destination", row.destination.as_str()),
                ("accessText", row.access_text.as_str()),
                ("propagation", row.propagation.as_str()),
            ] {
                insert(&mut map, key, value);
            }
            map.insert(QString::from("readOnly"), QVariant::from(&row.read_only));
            QVariant::from(&map)
        })
        .collect()
}

fn prune_candidate_rows(rows: &[VolumeRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "volumeName", &row.volume_name);
            insert(&mut map, "sizeText", &row.size_text);
            map.insert(QString::from("sizeKnown"), QVariant::from(&row.size_known));
            map.insert(
                QString::from("sizeBytes"),
                QVariant::from(&saturating_i64(row.size_bytes.unwrap_or_default())),
            );
            QVariant::from(&map)
        })
        .collect()
}

fn filtered_pairs(
    rows: &[VolumeKeyValueRow],
    query: &str,
    ascending: bool,
) -> Vec<VolumeKeyValueRow> {
    let mut rows: Vec<_> = rows
        .iter()
        .filter(|row| {
            query.is_empty()
                || row.key.to_lowercase().contains(query)
                || row.value.to_lowercase().contains(query)
        })
        .cloned()
        .collect();
    rows.sort_by(|left, right| {
        let order = left
            .key
            .to_lowercase()
            .cmp(&right.key.to_lowercase())
            .then_with(|| left.key.cmp(&right.key));
        if ascending { order } else { order.reverse() }
    });
    rows
}

fn local_destination(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("An export destination is required.".into());
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        let path = if rest.starts_with('/') {
            rest
        } else if let Some(path) = rest.strip_prefix("localhost/") {
            // Keep the leading slash stripped by `strip_prefix` explicit.
            return decoded_path(&format!("/{path}"));
        } else {
            return Err("The export destination must be a local file URL.".into());
        };
        percent_decode(path)?
    } else if let Some(path) = value.strip_prefix("file:") {
        percent_decode(path)?
    } else {
        if value.contains("://") {
            return Err("The export destination must be a local file path.".into());
        }
        // The current QML file dialog decodes selectedFile before invoking
        // Rust, so a plain path is already local text. Preserve literal '%'
        // characters instead of decoding it a second time.
        value.to_string()
    };
    if path.contains('\0') {
        return Err("The export destination is invalid.".into());
    }
    Ok(PathBuf::from(path))
}

fn decoded_path(value: &str) -> Result<PathBuf, String> {
    let value = percent_decode(value)?;
    if value.contains('\0') {
        Err("The export destination is invalid.".into())
    } else {
        Ok(PathBuf::from(value))
    }
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("The export destination contains an invalid URL escape.".into());
            }
            let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) else {
                return Err("The export destination contains an invalid URL escape.".into());
            };
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "The export destination is not valid UTF-8.".to_string())
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn safe_connection_message(status: i32) -> &'static str {
    match status {
        0 => "Connecting to Docker Engine…",
        2 => "Docker Engine is not available. Check that Docker is running and try again.",
        3 => "Permission denied while accessing Docker volumes. Check Docker socket permissions.",
        4 => "Docker could not be reached. Try again.",
        _ => "",
    }
}

fn safe_operation_error(error: &DockerError) -> &'static str {
    match error {
        DockerError::SocketNotFound(_)
        | DockerError::DaemonUnavailable(_)
        | DockerError::EngineUnavailable => "Docker Engine is not available.",
        DockerError::PermissionDenied => "Permission denied while accessing Docker volumes.",
        DockerError::VolumeNotFound(_) => "This volume no longer exists.",
        DockerError::VolumeInUse(_) | DockerError::Conflict(_) => {
            "Volume is still used by a container and cannot be removed."
        }
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => {
            "The Docker volume request timed out. Try again."
        }
        _ => "Docker could not complete the volume request. Try again.",
    }
}

fn sort_mode_from_name(name: &str) -> Option<VolumeSortMode> {
    Some(match name {
        "name_asc" => VolumeSortMode::NameAscending,
        "name_desc" => VolumeSortMode::NameDescending,
        "newest" => VolumeSortMode::NewestFirst,
        "oldest" => VolumeSortMode::OldestFirst,
        "largest" => VolumeSortMode::LargestFirst,
        "smallest" => VolumeSortMode::SmallestFirst,
        "most_containers" => VolumeSortMode::MostContainers,
        "fewest_containers" => VolumeSortMode::FewestContainers,
        "in_use_first" => VolumeSortMode::InUseFirst,
        "unused_first" => VolumeSortMode::UnusedFirst,
        _ => return None,
    })
}

fn sort_mode_name(mode: VolumeSortMode) -> &'static str {
    mode.as_str()
}

fn operation_name<'a>(state: &'a VolumesState, operation: &str) -> &'a str {
    state
        .operations
        .iter()
        .find(|(_, value)| value.as_str() == operation)
        .map(|(name, _)| name.as_str())
        .unwrap_or_default()
}

fn normalized_query(query: &QString) -> String {
    query.to_string().trim().to_lowercase()
}

fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn insert(map: &mut QVariantMap, key: &str, value: &str) {
    map.insert(QString::from(key), qv(value));
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn cancel(token: &mut Option<CancellationToken>) {
    if let Some(token) = token.take() {
        token.cancel();
    }
}

fn cancel_map(tokens: &mut HashMap<String, CancellationToken>) {
    for (_, token) in tokens.drain() {
        token.cancel();
    }
}

fn bump(generation: &mut u64) -> u64 {
    *generation = generation.wrapping_add(1);
    *generation
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn saturating_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_are_decoded_and_remote_urls_rejected() {
        assert_eq!(
            local_destination("file:///tmp/my%20volume.tar").unwrap(),
            PathBuf::from("/tmp/my volume.tar")
        );
        assert_eq!(
            local_destination("file://localhost/tmp/out.tar").unwrap(),
            PathBuf::from("/tmp/out.tar")
        );
        assert_eq!(
            local_destination("/tmp/100%.tar").unwrap(),
            PathBuf::from("/tmp/100%.tar")
        );
        assert!(local_destination("file://remote/tmp/out.tar").is_err());
        assert!(local_destination("https://example.invalid/out.tar").is_err());
        assert!(local_destination("file:///tmp/bad%2").is_err());
    }

    #[test]
    fn sort_names_match_current_qml_contract() {
        for name in [
            "name_asc",
            "name_desc",
            "newest",
            "oldest",
            "largest",
            "smallest",
            "most_containers",
            "fewest_containers",
            "in_use_first",
            "unused_first",
        ] {
            assert_eq!(sort_mode_name(sort_mode_from_name(name).unwrap()), name);
        }
        assert!(sort_mode_from_name("unknown").is_none());
    }

    #[test]
    fn connection_messages_never_echo_raw_connection_details() {
        assert!(!safe_connection_message(2).contains("/home/"));
        assert!(safe_connection_message(2).contains("Docker Engine"));
    }
}
