//! Docker Images QAbstractListModel/controller implementation.
//!
//! The CXX-Qt declaration lives in `resource_bridges.rs` so the existing
//! build-script input remains unchanged. This module supplies its Rust state
//! and implementation while network/volume bridges remain untouched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{PullImageOptions, RegistryAuth, RemoveImageOptions};

use crate::app_state::{get_services, map_docker_error};
use crate::bridge::resource_bridges::qobject;
use crate::controllers::images::{
    ImageDetailState, ImageSortMode, ImagesListState, ImagesState, SelectionChange,
};
use crate::models::image_model::{ImageDetailView, KeyValueRow, UsageRow};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

/// Rust state backing the one unified QML image model/controller.
#[derive(Default)]
pub struct ImageListModelRust {
    pub(crate) state: ImagesState,
    pub(crate) docker_ready: bool,
    pub(crate) search_query: QString,
    pub(crate) sort_mode: QString,
    pub(crate) status: i32,
    pub(crate) state_name: QString,
    pub(crate) status_text: QString,
    pub(crate) error_message: QString,
    pub(crate) error_kind: QString,
    pub(crate) loading: bool,
    pub(crate) count: i32,
    pub(crate) total_image_count: i32,
    pub(crate) in_use_count: i32,
    pub(crate) unused_count: i32,
    pub(crate) total_size_bytes: i64,
    pub(crate) total_size_text: QString,
    pub(crate) selected_image_id: QString,
    pub(crate) detail_loading: bool,
    pub(crate) detail_state: QString,
    pub(crate) detail_error: QString,
    pub(crate) detail_error_kind: QString,
    pub(crate) operation_in_progress: bool,
    pub(crate) removing_image_id: QString,
    pub(crate) remove_error_message: QString,
    pub(crate) pull_active: bool,
    pub(crate) pulling: bool,
    pub(crate) pull_status: QString,
    pub(crate) pull_error_message: QString,
    pub(crate) pull_progress_known: bool,
    pub(crate) pull_progress_text: QString,
    pub(crate) pull_layer_id: QString,
    pub(crate) pull_current: i64,
    pub(crate) pull_total: i64,
    pub(crate) pull_percent: f64,
    pub(crate) export_active: bool,
    pub(crate) exporting: bool,
    pub(crate) export_bytes_written: i64,
    pub(crate) export_bytes_text: QString,
    pub(crate) export_destination: QString,
    pub(crate) export_status: QString,
    pub(crate) export_error_message: QString,
    pub(crate) detail_id: QString,
    pub(crate) detail_short_id: QString,
    pub(crate) detail_display_name: QString,
    pub(crate) detail_tags: QString,
    pub(crate) detail_digests: QString,
    pub(crate) detail_created: QString,
    pub(crate) detail_size: QString,
    pub(crate) detail_virtual_size: QString,
    pub(crate) detail_platform: QString,
    pub(crate) detail_architecture: QString,
    pub(crate) detail_os: QString,
    pub(crate) detail_author: QString,
    pub(crate) detail_docker_version: QString,
    pub(crate) detail_comment: QString,
    pub(crate) detail_command: QString,
    pub(crate) detail_entrypoint: QString,
    pub(crate) detail_working_dir: QString,
    pub(crate) detail_user: QString,
    pub(crate) detail_stop_signal: QString,
    pub(crate) detail_shell: QString,
    pub(crate) detail: QVariant,
    pub(crate) environment_rows: QVariantList,
    pub(crate) label_rows: QVariantList,
    pub(crate) usage_rows: QVariantList,
    pub(crate) environment_model: QVariantList,
    pub(crate) label_model: QVariantList,
    pub(crate) usage_model: QVariantList,
    pub(crate) environment_search: String,
    pub(crate) environment_ascending: Option<bool>,
    pub(crate) label_search: String,
    pub(crate) label_ascending: Option<bool>,
    pub(crate) refresh_cancel: Option<CancellationToken>,
    pub(crate) detail_cancel: Option<CancellationToken>,
    pub(crate) remove_cancel: HashMap<String, CancellationToken>,
    pub(crate) pull_cancel: Option<CancellationToken>,
    pub(crate) export_cancel: Option<CancellationToken>,
}

impl Drop for ImageListModelRust {
    fn drop(&mut self) {
        if let Some(cancel) = self.refresh_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.detail_cancel.take() {
            cancel.cancel();
        }
        for (_, cancel) in self.remove_cancel.drain() {
            cancel.cancel();
        }
        if let Some(cancel) = self.pull_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.export_cancel.take() {
            cancel.cancel();
        }
    }
}

impl qobject::ImageListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.state.visible_rows.len() as i32
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.state.visible_rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        let operation = self
            .state
            .busy
            .get(&row.image_id)
            .cloned()
            .unwrap_or_default();
        match role {
            257 => qv(&row.image_id),
            258 => qv(&row.short_id),
            259 => qv(&row.display_name),
            260 => string_list(&row.repo_tags),
            261 => qv(&row.secondary_text),
            262 => QVariant::from(&(row.size_bytes.min(i64::MAX as u64) as i64)),
            263 => qv(&row.size_text),
            264 => qv(&row
                .created_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default()),
            265 => qv(&row.created_text),
            266 => qv(&row.architecture),
            267 => QVariant::from(&row.in_use),
            268 => QVariant::from(&(row.used_by_count.min(i32::MAX as usize) as i32)),
            269 => QVariant::from(&(row.image_id == self.state.selected_image_id)),
            270 => QVariant::from(&self.state.busy.contains_key(&row.image_id)),
            271 => qv(&operation),
            272 => qv(if row.in_use { "in_use" } else { "unused" }),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (257, "imageId"),
            (258, "shortId"),
            (259, "displayName"),
            (260, "repoTags"),
            (261, "secondaryText"),
            (262, "sizeBytes"),
            (263, "sizeText"),
            (264, "createdAt"),
            (265, "createdText"),
            (266, "architecture"),
            (267, "inUse"),
            (268, "usedByCount"),
            (269, "selected"),
            (270, "busy"),
            (271, "operation"),
            (272, "section"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    /// Initialize the Images controller exactly once. If Docker is already
    /// connected, initialization immediately starts the first list request;
    /// otherwise the ready connection transition starts it later.
    pub fn initialize(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if !state.initialize() {
            tracing::debug!("ImagesController initialize ignored; already initialized");
            return;
        }
        tracing::info!("ImagesPage created");
        tracing::info!("ImagesController initialized");
        self.as_mut().apply_state(state);
        if get_services().is_some() {
            self.as_mut().rust_mut().docker_ready = true;
            self.as_mut().refresh();
        } else {
            tracing::debug!("ImagesController waiting for Docker connection");
        }
    }

    /// Load summaries and all-container usage associations on Tokio.
    pub fn refresh(mut self: Pin<&mut Self>) {
        tracing::info!("Loading Docker images");
        if let Some(cancel) = self.as_mut().rust_mut().refresh_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
            cancel.cancel();
        }
        let Some(services) = get_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.begin_refresh();
            let error = tuxstack_docker_core::DockerError::EngineUnavailable;
            state.apply_list_error(generation, &error);
            self.as_mut().apply_state(state);
            return;
        };
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.begin_refresh();
            self.as_mut().apply_state(state);
            generation
        };
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().refresh_cancel = Some(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            // Search is deliberately absent here: it is always local GUI state.
            let options = tuxstack_docker_core::services::images::ListImagesOptions::default();
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.images.list_images(options) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let had_selection = !state.selected_image_id.is_empty();
                    let applied = match result {
                        Ok(images) => {
                            tracing::info!("Docker returned {} images", images.len());
                            state.apply_list(generation, &images)
                        }
                        Err(error) => {
                            tracing::debug!(%error, "Docker image list request failed");
                            state.apply_list_error(generation, &error)
                        }
                    };
                    if !applied {
                        return;
                    }
                    tracing::debug!(
                        row_count = state.source_rows.len(),
                        list_state = page_status_name(state.status),
                        "Updating image model"
                    );
                    model.as_mut().rust_mut().refresh_cancel = None;
                    let selected = state.selected_image_id.clone();
                    if !selected.is_empty() {
                        if had_selection {
                            tracing::debug!("Selecting preserved image");
                        } else {
                            tracing::info!("Selecting first image");
                        }
                    }
                    model.as_mut().apply_state(state);
                    if !selected.is_empty() {
                        model.as_mut().load_detail(&selected, false);
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "image refresh result dropped"));
        });
    }

    /// Apply local search immediately without another Docker request.
    pub(crate) fn update_search_query(mut self: Pin<&mut Self>, query: &QString) {
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_search_query(&query.to_string());
        self.as_mut().apply_state(state);
    }

    pub(crate) fn update_sort_mode(mut self: Pin<&mut Self>, mode: &QString) {
        let Some(mode) = sort_mode_from_name(&mode.to_string()) else {
            return;
        };
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_sort_mode(mode);
        self.as_mut().apply_state(state);
    }

    pub(crate) fn select_image(mut self: Pin<&mut Self>, image_id: &QString) {
        let image_id = image_id.to_string();
        self.as_mut().load_detail(&image_id, true);
    }

    pub(crate) fn reload_selected_image(mut self: Pin<&mut Self>) {
        let image_id = self.state.selected_image_id.clone();
        if !image_id.is_empty() {
            self.as_mut().load_detail(&image_id, false);
        }
    }

    pub(crate) fn set_connection_state(
        mut self: Pin<&mut Self>,
        docker_status: i32,
        message: &QString,
    ) {
        if !self.state.initialized {
            return;
        }
        if docker_status == 1 {
            if self.as_mut().rust_mut().docker_ready {
                return;
            }
            self.as_mut().rust_mut().docker_ready = true;
            self.as_mut().refresh();
            return;
        }
        self.as_mut().rust_mut().docker_ready = false;

        if let Some(cancel) = self.as_mut().rust_mut().refresh_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
            cancel.cancel();
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.source_rows.clear();
        state.visible_rows.clear();
        state.clear_selection();
        state.status = if docker_status == 0 {
            ImagesListState::Loading
        } else {
            ImagesListState::Error
        };
        state.error_kind = match docker_status {
            2 => "docker_unavailable",
            3 => "permission_denied",
            4 => "docker",
            _ => "",
        }
        .to_string();
        state.status_text = message.to_string();
        self.as_mut().apply_state(state);
    }

    fn load_detail(mut self: Pin<&mut Self>, image_id: &str, toggle: bool) {
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let change = if toggle {
                state.toggle_selection(image_id)
            } else {
                state
                    .select(image_id)
                    .map(SelectionChange::Selected)
                    .unwrap_or(SelectionChange::Ignored)
            };
            match change {
                SelectionChange::Selected(generation) => {
                    self.as_mut().apply_state(state);
                    generation
                }
                SelectionChange::Deselected => {
                    if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
                        cancel.cancel();
                    }
                    self.as_mut().apply_state(state);
                    return;
                }
                SelectionChange::Ignored => return,
            }
        };
        if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
            cancel.cancel();
        }
        let Some(services) = get_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            let (kind, message) =
                image_detail_error(&tuxstack_docker_core::DockerError::EngineUnavailable);
            state.apply_detail_error(generation, kind, message);
            self.as_mut().apply_state(state);
            return;
        };
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().detail_cancel = Some(cancel.clone());
        let image_id = image_id.to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.images.inspect_image(&image_id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match result {
                        Ok(detail) => {
                            tracing::debug!("Image detail loaded");
                            state.apply_detail(generation, &detail)
                        }
                        Err(error) => {
                            tracing::debug!(%error, image_id = %image_id, "image detail load failed");
                            let (kind, message) = image_detail_error(&error);
                            state.apply_detail_error(generation, kind, message)
                        }
                    };
                    if !applied {
                        return;
                    }
                    model.as_mut().rust_mut().detail_cancel = None;
                    model.as_mut().apply_state(state);
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "image detail result dropped"));
        });
    }

    pub(crate) fn remove_image(
        mut self: Pin<&mut Self>,
        image_id: &QString,
        force: bool,
        prune_children: bool,
    ) {
        let Some(services) = get_services() else {
            self.as_mut()
                .set_remove_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let image_id = image_id.to_string();
        let display_name = self
            .state
            .source_rows
            .iter()
            .find(|row| row.image_id == image_id)
            .map(|row| row.display_name.clone())
            .unwrap_or_else(|| image_id.clone());
        self.as_mut().set_remove_error_message(QString::default());
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_operation(&image_id, "removing") else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let cancel = CancellationToken::new();
        self.as_mut()
            .rust_mut()
            .remove_cancel
            .insert(image_id.clone(), cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => None,
                result = services.images.remove_image(
                    &image_id,
                    RemoveImageOptions {
                        force,
                        prune_children,
                    },
                ) => Some(result),
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    if !state.finish_operation(&image_id, generation) {
                        return;
                    }
                    model.as_mut().rust_mut().remove_cancel.remove(&image_id);
                    let (success, message) = match result {
                        None => (false, "Image removal cancelled".to_string()),
                        Some(Ok(_)) => {
                            let removed_selected = state.selected_image_id == image_id;
                            state.remove_local(&image_id);
                            if removed_selected
                                && let Some(cancel) = model.as_mut().rust_mut().detail_cancel.take()
                            {
                                cancel.cancel();
                            }
                            (true, "Image removed".to_string())
                        }
                        Some(Err(error)) => (false, map_docker_error(&error).user_message()),
                    };
                    model.as_mut().apply_state(state);
                    if success {
                        model.as_mut().image_removed(QString::from(&display_name));
                    } else {
                        model
                            .as_mut()
                            .set_remove_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from("remove"),
                        success,
                        QString::from(message),
                    );
                    if success {
                        model.as_mut().refresh();
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "image remove result dropped"));
        });
    }

    pub(crate) fn pull_image(
        mut self: Pin<&mut Self>,
        reference: &QString,
        platform: &QString,
        username: &QString,
        password: &QString,
        registry: &QString,
    ) {
        let Some(services) = get_services() else {
            self.as_mut()
                .set_pull_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let reference = reference.to_string();
        self.as_mut().set_pull_error_message(QString::default());
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_pull(&reference) else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let auth = if username.is_empty() && password.is_empty() && registry.is_empty() {
            None
        } else {
            Some(RegistryAuth {
                username: nonempty(username),
                password: nonempty(password),
                server_address: nonempty(registry),
                ..Default::default()
            })
        };
        let options = PullImageOptions {
            reference: reference.clone(),
            platform: nonempty(platform),
            registry_auth: auth,
        };
        let mut stream = services.images.pull_image(options);
        let cancel = stream.cancellation_token();
        self.as_mut().rust_mut().pull_cancel = Some(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let mut failure = None;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    item = stream.next() => match item {
                        Some(Ok(progress)) => {
                            let thread = qt_thread.clone();
                            if thread.queue(move |mut model| {
                                let mut state = model.as_mut().rust_mut().state.clone();
                                if state.apply_pull_progress(generation, &progress) {
                                    model.as_mut().apply_state(state);
                                }
                            }).is_err() {
                                break;
                            }
                        }
                        Some(Err(error)) => {
                            failure = Some(error);
                            break;
                        }
                        None => break,
                    }
                }
            }
            let cancelled = cancel.is_cancelled();
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let success = !cancelled && failure.is_none();
                    if !state.finish_pull(generation, success) {
                        return;
                    }
                    model.as_mut().rust_mut().pull_cancel = None;
                    model.as_mut().apply_state(state);
                    let (success, message) = if cancelled {
                        (false, "Image pull cancelled".to_string())
                    } else if let Some(error) = failure {
                        (false, map_docker_error(&error).user_message())
                    } else {
                        (true, format!("Pulled {reference}"))
                    };
                    if success {
                        model.as_mut().pull_completed(QString::from(&reference));
                    } else if !cancelled {
                        model
                            .as_mut()
                            .set_pull_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from("pull"),
                        success,
                        QString::from(message),
                    );
                    if success {
                        model.as_mut().refresh();
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "image pull completion dropped"));
        });
    }

    pub(crate) fn cancel_pull(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().pull_cancel.take() {
            cancel.cancel();
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        let generation = state.pull_generation;
        state.finish_pull(generation, false);
        self.as_mut().apply_state(state);
    }

    pub(crate) fn export_image(
        mut self: Pin<&mut Self>,
        image_id: &QString,
        destination: &QString,
    ) {
        let Some(services) = get_services() else {
            self.as_mut()
                .set_export_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let image_id = image_id.to_string();
        let destination = local_path(destination);
        let destination_text = destination.display().to_string();
        self.as_mut().set_export_error_message(QString::default());
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_export(&image_id, &destination_text) else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let mut stream = services.images.export_image(&image_id);
        let cancel = stream.cancellation_token();
        self.as_mut().rust_mut().export_cancel = Some(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let temporary = sibling_temporary_path(&destination, generation);
            let result = stream_export(
                &mut stream,
                &temporary,
                &destination,
                &cancel,
                generation,
                &qt_thread,
            )
            .await;
            let _ = tokio::fs::remove_file(&temporary).await;
            let cancelled = cancel.is_cancelled();
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    if !state.finish_export(generation) {
                        return;
                    }
                    model.as_mut().rust_mut().export_cancel = None;
                    model.as_mut().apply_state(state);
                    let (success, message) = match result {
                        Ok(bytes) => (
                            true,
                            format!(
                                "Image exported to {} ({})",
                                destination.display(),
                                crate::models::image_model::format_bytes(bytes)
                            ),
                        ),
                        Err(_message) if cancelled => (false, "Image export cancelled".to_string()),
                        Err(message) => (false, message),
                    };
                    if success {
                        model
                            .as_mut()
                            .export_completed(QString::from(&destination_text));
                    } else if !cancelled {
                        model
                            .as_mut()
                            .set_export_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from("export"),
                        success,
                        QString::from(message),
                    );
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "image export completion dropped"));
        });
    }

    pub(crate) fn cancel_export(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().export_cancel.take() {
            cancel.cancel();
        }
        let mut state = self.as_mut().rust_mut().state.clone();
        let generation = state.export_generation;
        state.finish_export(generation);
        self.as_mut().apply_state(state);
    }

    pub(crate) fn open_container(mut self: Pin<&mut Self>, container_id: &QString) {
        self.as_mut()
            .container_navigation_requested(container_id.clone());
    }

    pub(crate) fn set_environment_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().environment_search = query.to_string().trim().to_lowercase();
        self.as_mut().sync_filtered_tables();
    }

    pub(crate) fn set_environment_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().environment_ascending = Some(ascending);
        self.as_mut().sync_filtered_tables();
    }

    pub(crate) fn set_label_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().label_search = query.to_string().trim().to_lowercase();
        self.as_mut().sync_filtered_tables();
    }

    pub(crate) fn set_label_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().label_ascending = Some(ascending);
        self.as_mut().sync_filtered_tables();
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().refresh_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
            cancel.cancel();
        }
        for (_, cancel) in self.as_mut().rust_mut().remove_cancel.drain() {
            cancel.cancel();
        }
        self.as_mut().cancel_pull();
        self.as_mut().cancel_export();
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.detail_generation = state.detail_generation.wrapping_add(1);
        state.operation_generation = state.operation_generation.wrapping_add(1);
        state.pull_generation = state.pull_generation.wrapping_add(1);
        state.export_generation = state.export_generation.wrapping_add(1);
        state.busy.clear();
        state.busy_generations.clear();
        state.operation_in_progress = false;
        self.as_mut().apply_state(state);
    }

    /// Replace model state and synchronize all scalar/structured properties.
    fn apply_state(mut self: Pin<&mut Self>, state: ImagesState) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().state = state;
        self.as_mut().end_reset_model();
        let state = self.state.clone();
        self.as_mut()
            .set_search_query(QString::from(&state.search_query));
        self.as_mut()
            .set_sort_mode(QString::from(sort_mode_name(state.sort_mode)));
        self.as_mut().set_status(state.status as i32);
        self.as_mut()
            .set_state_name(QString::from(page_status_name(state.status)));
        self.as_mut()
            .set_status_text(QString::from(&state.status_text));
        self.as_mut()
            .set_error_message(QString::from(&state.status_text));
        self.as_mut()
            .set_error_kind(QString::from(&state.error_kind));
        self.as_mut()
            .set_loading(state.status == ImagesListState::Loading);
        self.as_mut()
            .set_count(saturating_i32(state.visible_rows.len()));
        self.as_mut()
            .set_total_image_count(saturating_i32(state.total_image_count()));
        self.as_mut()
            .set_in_use_count(saturating_i32(state.in_use_count()));
        self.as_mut()
            .set_unused_count(saturating_i32(state.unused_count()));
        self.as_mut()
            .set_total_size_bytes(saturating_i64(state.total_size_bytes()));
        self.as_mut()
            .set_total_size_text(QString::from(state.total_size_text()));
        self.as_mut()
            .set_selected_image_id(QString::from(&state.selected_image_id));
        self.as_mut()
            .set_detail_loading(state.detail_status == ImageDetailState::Loading);
        self.as_mut()
            .set_detail_state(QString::from(detail_status_name(state.detail_status)));
        self.as_mut()
            .set_detail_error(QString::from(&state.detail_error));
        self.as_mut()
            .set_detail_error_kind(QString::from(&state.detail_error_kind));
        self.as_mut()
            .set_operation_in_progress(state.operation_in_progress);
        let removing = state
            .busy
            .iter()
            .find(|(_, operation)| operation.as_str() == "removing")
            .map(|(id, _)| id.as_str())
            .unwrap_or_default();
        self.as_mut().set_removing_image_id(QString::from(removing));
        self.as_mut().set_pull_active(state.pull.active);
        self.as_mut().set_pulling(state.pull.active);
        self.as_mut()
            .set_pull_status(QString::from(&state.pull.status));
        self.as_mut().set_pull_progress_known(state.pull.total > 0);
        self.as_mut()
            .set_pull_progress_text(QString::from(if state.pull.current > 0 {
                format!(
                    "{} / {}",
                    crate::models::image_model::format_bytes(state.pull.current),
                    if state.pull.total > 0 {
                        crate::models::image_model::format_bytes(state.pull.total)
                    } else {
                        "unknown".to_string()
                    }
                )
            } else {
                String::new()
            }));
        self.as_mut()
            .set_pull_layer_id(QString::from(&state.pull.layer_id));
        self.as_mut()
            .set_pull_current(saturating_i64(state.pull.current));
        self.as_mut()
            .set_pull_total(saturating_i64(state.pull.total));
        self.as_mut().set_pull_percent(state.pull.percent.max(0.0));
        self.as_mut().set_export_active(state.export.active);
        self.as_mut().set_exporting(state.export.active);
        self.as_mut()
            .set_export_bytes_written(saturating_i64(state.export.bytes_written));
        self.as_mut().set_export_bytes_text(QString::from(
            crate::models::image_model::format_bytes(state.export.bytes_written),
        ));
        self.as_mut()
            .set_export_destination(QString::from(&state.export.destination));
        self.as_mut()
            .set_export_status(QString::from(if state.export.active {
                "Exporting image"
            } else {
                ""
            }));
        self.as_mut().sync_detail(state.detail.as_ref());
    }

    fn sync_detail(mut self: Pin<&mut Self>, detail: Option<&ImageDetailView>) {
        let empty = ImageDetailView::default();
        let detail = detail.unwrap_or(&empty);
        self.as_mut().set_detail_id(QString::from(&detail.image_id));
        self.as_mut()
            .set_detail_short_id(QString::from(&detail.short_id));
        self.as_mut()
            .set_detail_display_name(QString::from(&detail.display_name));
        self.as_mut()
            .set_detail_tags(QString::from(&detail.tags_text));
        self.as_mut()
            .set_detail_digests(QString::from(&detail.digests_text));
        self.as_mut()
            .set_detail_created(QString::from(&detail.created_text));
        self.as_mut()
            .set_detail_size(QString::from(&detail.size_text));
        self.as_mut()
            .set_detail_virtual_size(QString::from(&detail.virtual_size_text));
        self.as_mut()
            .set_detail_platform(QString::from(&detail.platform));
        self.as_mut()
            .set_detail_architecture(QString::from(&detail.architecture));
        self.as_mut().set_detail_os(QString::from(&detail.os));
        self.as_mut()
            .set_detail_author(QString::from(&detail.author));
        self.as_mut()
            .set_detail_docker_version(QString::from(&detail.docker_version));
        self.as_mut()
            .set_detail_comment(QString::from(&detail.comment));
        self.as_mut()
            .set_detail_command(QString::from(&detail.command));
        self.as_mut()
            .set_detail_entrypoint(QString::from(&detail.entrypoint));
        self.as_mut()
            .set_detail_working_dir(QString::from(&detail.working_dir));
        self.as_mut().set_detail_user(QString::from(&detail.user));
        self.as_mut()
            .set_detail_stop_signal(QString::from(&detail.stop_signal));
        self.as_mut().set_detail_shell(QString::from(&detail.shell));
        self.as_mut().set_detail(detail_variant(detail));
        let environment = key_value_rows(&detail.environment);
        let labels = key_value_rows(&detail.labels);
        let usage = usage_rows(&detail.usage);
        self.as_mut().set_environment_rows(environment);
        self.as_mut().set_label_rows(labels);
        self.as_mut().set_usage_rows(usage.clone());
        self.as_mut().set_usage_model(usage);
        self.as_mut().sync_filtered_tables();
    }

    fn sync_filtered_tables(mut self: Pin<&mut Self>) {
        let Some(detail) = self.state.detail.clone() else {
            self.as_mut().set_environment_model(QVariantList::default());
            self.as_mut().set_label_model(QVariantList::default());
            return;
        };
        let environment = filtered_rows(
            &detail.environment,
            &self.environment_search,
            self.environment_ascending.unwrap_or(true),
        );
        let labels = filtered_rows(
            &detail.labels,
            &self.label_search,
            self.label_ascending.unwrap_or(true),
        );
        self.as_mut()
            .set_environment_model(key_value_rows(&environment));
        self.as_mut().set_label_model(key_value_rows(&labels));
    }
}

async fn stream_export(
    stream: &mut tuxstack_docker_core::streams::ImageExportStream,
    temporary: &Path,
    destination: &Path,
    cancel: &CancellationToken,
    generation: u64,
    qt_thread: &cxx_qt::CxxQtThread<qobject::ImageListModel>,
) -> Result<u64, String> {
    // The SaveFile dialog owns overwrite confirmation. The bridge writes only
    // to this sibling and performs one final atomic replacement.
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temporary)
        .await
        .map_err(io_message)?;
    let mut written = 0_u64;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err("Image export cancelled".to_string()),
            item = stream.next() => match item {
                Some(Ok(chunk)) => {
                    file.write_all(&chunk).await.map_err(io_message)?;
                    written = written.saturating_add(chunk.len() as u64);
                    let thread = qt_thread.clone();
                    let _ = thread.queue(move |mut model| {
                        let mut state = model.as_mut().rust_mut().state.clone();
                        if state.update_export_bytes(generation, written) {
                            model.as_mut().apply_state(state);
                        }
                    });
                }
                Some(Err(error)) => return Err(map_docker_error(&error).user_message()),
                None => break,
            }
        }
    }
    file.flush().await.map_err(io_message)?;
    file.sync_all().await.map_err(io_message)?;
    drop(file);
    tokio::fs::rename(temporary, destination)
        .await
        .map_err(io_message)?;
    Ok(written)
}

fn detail_variant(detail: &ImageDetailView) -> QVariant {
    if detail.image_id.is_empty() {
        return QVariant::default();
    }
    let mut map = QVariantMap::default();
    map.insert(QString::from("repoTags"), string_list(&detail.repo_tags));
    for (key, value) in [
        ("imageId", detail.image_id.as_str()),
        ("shortId", detail.short_id.as_str()),
        ("displayName", detail.display_name.as_str()),
        ("tagsText", detail.tags_text.as_str()),
        ("repoDigestsText", detail.digests_text.as_str()),
        ("createdText", detail.created_text.as_str()),
        ("createdFullText", detail.created_full_text.as_str()),
        ("sizeText", detail.size_text.as_str()),
        ("virtualSizeText", detail.virtual_size_text.as_str()),
        ("platform", detail.platform.as_str()),
        ("architecture", detail.architecture.as_str()),
        ("os", detail.os.as_str()),
        ("author", detail.author.as_str()),
        ("dockerVersion", detail.docker_version.as_str()),
        ("comment", detail.comment.as_str()),
        ("commandText", detail.command.as_str()),
        ("entrypointText", detail.entrypoint.as_str()),
        ("workingDir", detail.working_dir.as_str()),
        ("user", detail.user.as_str()),
        ("stopSignal", detail.stop_signal.as_str()),
        ("hostname", "—"),
        ("domainName", "—"),
        ("shellText", detail.shell.as_str()),
    ] {
        insert(&mut map, key, value);
    }
    QVariant::from(&map)
}

fn filtered_rows(rows: &[KeyValueRow], query: &str, ascending: bool) -> Vec<KeyValueRow> {
    let mut filtered: Vec<_> = rows
        .iter()
        .filter(|row| {
            query.is_empty()
                || row.key.to_lowercase().contains(query)
                || row.value.to_lowercase().contains(query)
        })
        .cloned()
        .collect();
    filtered.sort_by(|left, right| {
        let order = left
            .key
            .to_lowercase()
            .cmp(&right.key.to_lowercase())
            .then_with(|| left.value.cmp(&right.value));
        if ascending { order } else { order.reverse() }
    });
    filtered
}

fn key_value_rows(rows: &[KeyValueRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "key", &row.key);
            insert(&mut map, "value", &row.value);
            QVariant::from(&map)
        })
        .collect()
}

fn usage_rows(rows: &[UsageRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "containerId", &row.container_id);
            insert(&mut map, "shortId", &row.short_id);
            insert(&mut map, "name", &row.name);
            insert(&mut map, "state", &row.state);
            insert(&mut map, "status", &row.status);
            insert(&mut map, "createdText", &row.created_at);
            QVariant::from(&map)
        })
        .collect()
}

fn insert(map: &mut QVariantMap, key: &str, value: &str) {
    map.insert(QString::from(key), qv(value));
}

fn string_list(values: &[String]) -> QVariant {
    let list: QVariantList = values.iter().map(|value| qv(value)).collect();
    QVariant::from(&list)
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn sort_mode_from_name(name: &str) -> Option<ImageSortMode> {
    Some(match name {
        "name_asc" => ImageSortMode::NameAscending,
        "name_desc" => ImageSortMode::NameDescending,
        "newest" => ImageSortMode::NewestFirst,
        "oldest" => ImageSortMode::OldestFirst,
        "largest" => ImageSortMode::LargestFirst,
        "smallest" => ImageSortMode::SmallestFirst,
        "used_first" => ImageSortMode::UsedFirst,
        "unused_first" => ImageSortMode::UnusedFirst,
        _ => return None,
    })
}

fn sort_mode_name(mode: ImageSortMode) -> &'static str {
    match mode {
        ImageSortMode::NameAscending => "name_asc",
        ImageSortMode::NameDescending => "name_desc",
        ImageSortMode::NewestFirst => "newest",
        ImageSortMode::OldestFirst => "oldest",
        ImageSortMode::LargestFirst => "largest",
        ImageSortMode::SmallestFirst => "smallest",
        ImageSortMode::UsedFirst => "used_first",
        ImageSortMode::UnusedFirst => "unused_first",
    }
}

fn image_detail_error(error: &tuxstack_docker_core::DockerError) -> (String, String) {
    use tuxstack_docker_core::DockerError;

    let (kind, message) = match error {
        DockerError::SocketNotFound(_) | DockerError::EngineUnavailable => (
            "docker_unavailable",
            "Docker Engine is unavailable. Check that the Docker daemon is running.",
        ),
        DockerError::PermissionDenied => (
            "permission_denied",
            "Permission denied while accessing Docker. Check Docker socket permissions.",
        ),
        DockerError::ImageNotFound(_) => (
            "image_not_found",
            "This image no longer exists. Refresh the image list.",
        ),
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => {
            ("timeout", "Loading image details timed out. Try again.")
        }
        _ => (
            "docker",
            "Docker returned an error while loading image details.",
        ),
    };
    (kind.to_string(), message.to_string())
}

fn page_status_name(status: ImagesListState) -> &'static str {
    match status {
        ImagesListState::Loading => "loading",
        ImagesListState::Ready => "ready",
        ImagesListState::Empty => "empty",
        ImagesListState::Error => "error",
    }
}

fn detail_status_name(status: ImageDetailState) -> &'static str {
    match status {
        ImageDetailState::None => "none",
        ImageDetailState::Loading => "loading",
        ImageDetailState::Ready => "ready",
        ImageDetailState::Error => "error",
    }
}

fn nonempty(value: &QString) -> Option<String> {
    let value = value.to_string();
    (!value.trim().is_empty()).then_some(value)
}

fn local_path(value: &QString) -> PathBuf {
    let value = value.to_string();
    // FileDialog normally returns a file URL. Decode common URL escapes without
    // introducing synchronous Qt/file work or another dependency.
    let value = value.strip_prefix("file://").unwrap_or(&value);
    PathBuf::from(percent_decode(value))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn sibling_temporary_path(destination: &Path, generation: u64) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image.tar");
    destination.with_file_name(format!(
        ".{name}.tuxstack-{}-{generation}.part",
        std::process::id()
    ))
}

fn io_message(error: std::io::Error) -> String {
    if error.raw_os_error() == Some(28) {
        "Image export failed: destination is out of disk space".to_string()
    } else if error.kind() == std::io::ErrorKind::PermissionDenied {
        "Image export failed: destination permission denied".to_string()
    } else {
        format!("Image export failed: {error}")
    }
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
    fn file_url_and_temporary_path_are_safe() {
        let path = local_path(&QString::from("file:///tmp/my%20image.tar"));
        assert_eq!(path, PathBuf::from("/tmp/my image.tar"));
        assert_eq!(
            sibling_temporary_path(&path, 7),
            PathBuf::from(format!(
                "/tmp/.my image.tar.tuxstack-{}-7.part",
                std::process::id()
            ))
        );
    }

    #[test]
    fn image_detail_errors_are_safe_and_specific() {
        use tuxstack_docker_core::DockerError;

        assert_eq!(
            image_detail_error(&DockerError::ImageNotFound("secret-image".into())),
            (
                "image_not_found".into(),
                "This image no longer exists. Refresh the image list.".into()
            )
        );
        assert_eq!(
            image_detail_error(&DockerError::OperationTimeout).0,
            "timeout"
        );
        assert_eq!(
            image_detail_error(&DockerError::PermissionDenied).0,
            "permission_denied"
        );
        let (_, message) = image_detail_error(&DockerError::Api(
            "daemon leaked implementation details".into(),
        ));
        assert!(!message.contains("leaked implementation details"));
    }
}
