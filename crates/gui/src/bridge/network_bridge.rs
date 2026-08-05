//! Docker Networks QAbstractListModel/controller implementation.
//!
//! The CXX-Qt declaration remains in `resource_bridges.rs`; this module owns
//! list/detail state, asynchronous Docker work, cancellation, and QVariant
//! conversion without exposing Bollard DTOs to QML.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;
use tuxstack_client::{DaemonError as DockerError, ListNetworksOptions};
use tuxstack_domain::CreateNetworkOptions;

use crate::app_state::daemon_services;
use crate::bridge::resource_bridges::qobject;
use crate::controllers::networks::{
    NetworkDetailState, NetworkOperationKind, NetworkSortMode, NetworksListState, NetworksState,
};
use crate::models::network_model::{
    NetworkContainerView, NetworkDetailView, NetworkKeyValueRow, NetworkSubnetView,
};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

const CREATE_ENABLE_IPV6: i32 = 1;
const CREATE_INTERNAL: i32 = 2;
const CREATE_ATTACHABLE: i32 = 4;

#[derive(Default)]
pub struct NetworkListModelRust {
    pub(crate) state: NetworksState,
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
    pub(crate) total_network_count: i32,
    pub(crate) selected_network_id: QString,
    pub(crate) detail_loading: bool,
    pub(crate) detail_state: QString,
    pub(crate) detail_error: QString,
    pub(crate) detail_error_kind: QString,
    pub(crate) detail: QVariant,
    pub(crate) option_rows: QVariantList,
    pub(crate) label_rows: QVariantList,
    pub(crate) subnet_rows: QVariantList,
    pub(crate) container_rows: QVariantList,
    pub(crate) operation_in_progress: bool,
    pub(crate) creating: bool,
    pub(crate) create_error_message: QString,
    pub(crate) remove_preparation_active: bool,
    pub(crate) removing_network_id: QString,
    pub(crate) remove_error_message: QString,
    pub(crate) refresh_cancel: Option<CancellationToken>,
    pub(crate) detail_cancel: Option<CancellationToken>,
    pub(crate) create_cancel: Option<CancellationToken>,
    pub(crate) remove_prepare_cancel: Option<CancellationToken>,
    pub(crate) remove_cancel: Option<CancellationToken>,
    pub(crate) remove_prepare_generation: u64,
}

impl Drop for NetworkListModelRust {
    fn drop(&mut self) {
        cancel(&mut self.refresh_cancel);
        cancel(&mut self.detail_cancel);
        cancel(&mut self.create_cancel);
        cancel(&mut self.remove_prepare_cancel);
        cancel(&mut self.remove_cancel);
    }
}

impl qobject::NetworkListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.state.visible_rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.state.visible_rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        let operation =
            if self.state.operation.active && self.state.operation.network_id == row.network_id {
                match self.state.operation.kind {
                    Some(NetworkOperationKind::Remove) => "removing",
                    Some(NetworkOperationKind::Create) | None => "",
                }
            } else {
                ""
            };
        match role {
            257 => qv(&row.network_id),
            258 => qv(&row.short_id),
            259 => qv(&row.name),
            260 => qv(&row.subnet),
            261 => qv(&row.gateway),
            262 => qv(&row.driver),
            263 => qv(&row.scope),
            264 => qv(&row
                .created_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default()),
            265 => qv(&row.created_text),
            266 => QVariant::from(&row.internal),
            267 => QVariant::from(&row.attachable),
            268 => QVariant::from(&row.ingress),
            269 => QVariant::from(&row.ipv4),
            270 => QVariant::from(&row.ipv6),
            271 => QVariant::from(&(row.network_id == self.state.selected_network_id)),
            272 => QVariant::from(&(!operation.is_empty())),
            273 => qv(operation),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (257, "networkId"),
            (258, "shortId"),
            (259, "name"),
            (260, "subnet"),
            (261, "gateway"),
            (262, "driver"),
            (263, "scope"),
            (264, "createdAt"),
            (265, "createdText"),
            (266, "internal"),
            (267, "attachable"),
            (268, "ingress"),
            (269, "ipv4"),
            (270, "ipv6"),
            (271, "selected"),
            (272, "busy"),
            (273, "operation"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    pub(crate) fn initialize(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if !state.initialize() {
            tracing::debug!("NetworksController initialize ignored; already initialized");
            return;
        }
        tracing::info!("NetworksPage created");
        tracing::info!("NetworksController initialized");
        self.as_mut().apply_state(state);
        if daemon_services().is_some() {
            self.as_mut().rust_mut().docker_ready = true;
            self.as_mut().refresh();
        } else {
            tracing::debug!("NetworksController waiting for Docker connection");
        }
    }

    pub(crate) fn refresh(mut self: Pin<&mut Self>) {
        tracing::info!("Loading Docker networks");
        cancel(&mut self.as_mut().rust_mut().refresh_cancel);
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        cancel(&mut self.as_mut().rust_mut().remove_prepare_cancel);
        self.as_mut().rust_mut().remove_prepare_generation = self
            .as_mut()
            .rust_mut()
            .remove_prepare_generation
            .wrapping_add(1);
        self.as_mut().set_remove_preparation_active(false);
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.begin_refresh();
            self.as_mut().apply_state(state);
            generation
        };
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_list_error(generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().refresh_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let options = ListNetworksOptions::default();
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.networks.list_networks(&options) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let had_selection = !state.selected_network_id.is_empty();
                    let applied = match result {
                        Ok(networks) => {
                            tracing::info!("Docker returned {} networks", networks.len());
                            state.apply_list(generation, &networks)
                        }
                        Err(error) => {
                            tracing::debug!(%error, "Docker network list request failed");
                            state.apply_list_error(generation, &error)
                        }
                    };
                    if !applied {
                        return;
                    }
                    model.as_mut().rust_mut().refresh_cancel = None;
                    let selected = state.selected_network_id.clone();
                    tracing::debug!(
                        row_count = state.source_rows.len(),
                        list_state = list_state_name(state.status),
                        "Updating network model"
                    );
                    if !selected.is_empty() {
                        if had_selection {
                            tracing::debug!("Selecting preserved network");
                        } else {
                            tracing::info!("Selecting first network");
                        }
                    }
                    model.as_mut().apply_state(state);
                    if !selected.is_empty() {
                        model.as_mut().load_detail(&selected);
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "network refresh result dropped"));
        });
    }

    pub(crate) fn update_search_query(mut self: Pin<&mut Self>, query: &QString) {
        let previous = self.state.selected_network_id.clone();
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_search_query(&query.to_string());
        let selected = state.selected_network_id.clone();
        let selection_changed = selected != previous;
        self.as_mut().apply_state(state);
        if selection_changed {
            cancel(&mut self.as_mut().rust_mut().detail_cancel);
            if !selected.is_empty() {
                self.as_mut().load_detail(&selected);
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

    pub(crate) fn select_network(mut self: Pin<&mut Self>, network_id: &QString) {
        self.as_mut().load_detail(&network_id.to_string());
    }

    pub(crate) fn reload_selected_network(mut self: Pin<&mut Self>) {
        let network_id = self.state.selected_network_id.clone();
        if !network_id.is_empty() {
            self.as_mut().load_detail(&network_id);
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
        self.as_mut().cancel_all();
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.detail_generation = state.detail_generation.wrapping_add(1);
        state.operation.generation = state.operation.generation.wrapping_add(1);
        state.operation.active = false;
        state.operation.kind = None;
        state.operation.network_id.clear();
        state.source_rows.clear();
        state.visible_rows.clear();
        state.clear_selection();
        state.status = if docker_status == 0 {
            NetworksListState::Loading
        } else {
            NetworksListState::Error
        };
        state.error_kind = match docker_status {
            2 => "docker_unavailable",
            3 => "permission_denied",
            4 => "docker",
            _ => "",
        }
        .into();
        state.status_text = safe_connection_message(docker_status, &message.to_string());
        self.as_mut().apply_state(state);
    }

    fn load_detail(mut self: Pin<&mut Self>, network_id: &str) {
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = if state.selected_network_id == network_id {
                state.begin_selected_inspect()
            } else {
                state.select(network_id)
            };
            let Some(generation) = generation else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_detail_error(generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        let token = CancellationToken::new();
        self.as_mut().rust_mut().detail_cancel = Some(token.clone());
        let network_id = network_id.to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.networks.inspect_network(&network_id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match result {
                        Ok(detail) => {
                            tracing::debug!(network_id = %network_id, "Network detail loaded");
                            state.apply_detail(generation, &detail)
                        }
                        Err(error) => {
                            tracing::debug!(%error, network_id = %network_id, "network detail load failed");
                            state.apply_detail_error(generation, &error)
                        }
                    };
                    if applied {
                        model.as_mut().rust_mut().detail_cancel = None;
                        model.as_mut().apply_state(state);
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "network detail result dropped"));
        });
    }

    pub(crate) fn create_network(
        mut self: Pin<&mut Self>,
        name: &QString,
        driver: &QString,
        subnet: &QString,
        gateway: &QString,
        flags: i32,
        labels_text: &QString,
    ) {
        self.as_mut().set_create_error_message(QString::default());
        let options = match create_options(
            &name.to_string(),
            &driver.to_string(),
            &subnet.to_string(),
            &gateway.to_string(),
            flags & CREATE_ENABLE_IPV6 != 0,
            flags & CREATE_INTERNAL != 0,
            flags & CREATE_ATTACHABLE != 0,
            &labels_text.to_string(),
        ) {
            Ok(options) => options,
            Err(message) => {
                self.as_mut()
                    .set_create_error_message(QString::from(message));
                return;
            }
        };
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_create() else {
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.fail_operation(generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            self.as_mut()
                .set_create_error_message(QString::from("Docker Engine is not available."));
            return;
        };
        cancel(&mut self.as_mut().rust_mut().create_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().create_cancel = Some(token.clone());
        let name = options.name.clone();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.networks.create_network(options) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let (success, message) = match result {
                        Ok(created) if state.finish_create(generation, &created.id) => {
                            let message =
                                created.warning.unwrap_or_else(|| "Network created".into());
                            (true, message)
                        }
                        Ok(_) => return,
                        Err(error) => {
                            tracing::debug!(%error, "network creation failed");
                            if !state.fail_operation(generation, &error) {
                                return;
                            }
                            (false, state.operation.error_message.clone())
                        }
                    };
                    model.as_mut().rust_mut().create_cancel = None;
                    model.as_mut().apply_state(state);
                    if success {
                        model.as_mut().network_created(QString::from(&name));
                    } else {
                        model
                            .as_mut()
                            .set_create_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from("create"),
                        success,
                        QString::from(&message),
                    );
                    if success {
                        model.as_mut().refresh();
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "network create result dropped"));
        });
    }

    pub(crate) fn prepare_remove_network(mut self: Pin<&mut Self>, network_id: &QString) {
        cancel(&mut self.as_mut().rust_mut().remove_prepare_cancel);
        let Some(row) = self
            .state
            .source_rows
            .iter()
            .find(|row| row.network_id == network_id.to_string())
            .cloned()
        else {
            return;
        };
        let Some(services) = daemon_services() else {
            self.as_mut()
                .remove_preparation_failed(QString::from("Docker Engine is not available."));
            return;
        };
        self.as_mut().rust_mut().remove_prepare_generation = self
            .as_mut()
            .rust_mut()
            .remove_prepare_generation
            .wrapping_add(1);
        let generation = self.remove_prepare_generation;
        self.as_mut().set_remove_preparation_active(true);
        self.as_mut().set_remove_error_message(QString::default());
        let token = CancellationToken::new();
        self.as_mut().rust_mut().remove_prepare_cancel = Some(token.clone());
        let network_id = row.network_id.clone();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.networks.inspect_network(&network_id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    if generation != model.remove_prepare_generation {
                        return;
                    }
                    model.as_mut().rust_mut().remove_prepare_cancel = None;
                    model.as_mut().set_remove_preparation_active(false);
                    match result {
                        Ok(detail) => model.as_mut().remove_prepared(
                            QString::from(&row.network_id),
                            QString::from(&row.name),
                            QString::from(&row.short_id),
                            saturating_i32(detail.containers.len()),
                        ),
                        Err(error) => {
                            tracing::debug!(%error, network_id = %network_id, "remove preparation inspect failed");
                            model.as_mut().remove_preparation_failed(QString::from(
                                friendly_operation_error(&error),
                            ));
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "remove preparation result dropped"));
        });
    }

    pub(crate) fn remove_network(mut self: Pin<&mut Self>, network_id: &QString) {
        let network_id = network_id.to_string();
        let name = self
            .state
            .source_rows
            .iter()
            .find(|row| row.network_id == network_id)
            .map(|row| row.name.clone())
            .unwrap_or_else(|| network_id.clone());
        self.as_mut().set_remove_error_message(QString::default());
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_remove(&network_id) else {
                self.as_mut().set_remove_error_message(QString::from(
                    "This network no longer exists. Refresh the network list.",
                ));
                return;
            };
            self.as_mut().apply_state(state);
            generation
        };
        let Some(services) = daemon_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.fail_operation(generation, &DockerError::EngineUnavailable);
            let message = state.operation.error_message.clone();
            self.as_mut().apply_state(state);
            self.as_mut()
                .set_remove_error_message(QString::from(message));
            return;
        };
        cancel(&mut self.as_mut().rust_mut().remove_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().remove_cancel = Some(token.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.networks.remove_network(&network_id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let (success, message) = match result {
                        Ok(()) if state.finish_remove(generation, &network_id) => {
                            if state.selected_network_id == network_id {
                                cancel(&mut model.as_mut().rust_mut().detail_cancel);
                            }
                            state.remove_local(&network_id);
                            (true, "Network removed".to_string())
                        }
                        Ok(()) => return,
                        Err(error) => {
                            tracing::debug!(%error, network_id = %network_id, "network removal failed");
                            if !state.fail_operation(generation, &error) {
                                return;
                            }
                            (false, state.operation.error_message.clone())
                        }
                    };
                    model.as_mut().rust_mut().remove_cancel = None;
                    model.as_mut().apply_state(state);
                    if success {
                        model.as_mut().network_removed(QString::from(&name));
                    } else {
                        model.as_mut().set_remove_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from("remove"),
                        success,
                        QString::from(&message),
                    );
                    if success {
                        model.as_mut().refresh();
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "network remove result dropped"));
        });
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_all();
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.detail_generation = state.detail_generation.wrapping_add(1);
        state.operation.generation = state.operation.generation.wrapping_add(1);
        state.operation.active = false;
        state.operation.kind = None;
        state.operation.network_id.clear();
        self.as_mut().apply_state(state);
    }

    fn cancel_all(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().refresh_cancel);
        cancel(&mut self.as_mut().rust_mut().detail_cancel);
        cancel(&mut self.as_mut().rust_mut().create_cancel);
        cancel(&mut self.as_mut().rust_mut().remove_prepare_cancel);
        cancel(&mut self.as_mut().rust_mut().remove_cancel);
        self.as_mut().rust_mut().remove_prepare_generation = self
            .as_mut()
            .rust_mut()
            .remove_prepare_generation
            .wrapping_add(1);
        self.as_mut().set_remove_preparation_active(false);
    }

    fn apply_state(mut self: Pin<&mut Self>, state: NetworksState) {
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
            .set_state_name(QString::from(list_state_name(state.status)));
        self.as_mut()
            .set_status_text(QString::from(&state.status_text));
        self.as_mut()
            .set_error_message(QString::from(&state.status_text));
        self.as_mut()
            .set_error_kind(QString::from(&state.error_kind));
        self.as_mut()
            .set_loading(state.status == NetworksListState::Loading);
        self.as_mut()
            .set_count(saturating_i32(state.visible_rows.len()));
        self.as_mut()
            .set_total_network_count(saturating_i32(state.total_network_count()));
        self.as_mut()
            .set_selected_network_id(QString::from(&state.selected_network_id));
        self.as_mut()
            .set_detail_loading(state.detail_status == NetworkDetailState::Loading);
        self.as_mut()
            .set_detail_state(QString::from(detail_state_name(state.detail_status)));
        self.as_mut()
            .set_detail_error(QString::from(&state.detail_error));
        self.as_mut()
            .set_detail_error_kind(QString::from(&state.detail_error_kind));
        self.as_mut()
            .set_operation_in_progress(state.operation.active);
        self.as_mut().set_creating(
            state.operation.active && state.operation.kind == Some(NetworkOperationKind::Create),
        );
        let removing = if state.operation.active
            && state.operation.kind == Some(NetworkOperationKind::Remove)
        {
            state.operation.network_id.as_str()
        } else {
            ""
        };
        self.as_mut()
            .set_removing_network_id(QString::from(removing));
        self.as_mut().sync_detail(state.detail.as_ref());
    }

    fn sync_detail(mut self: Pin<&mut Self>, detail: Option<&NetworkDetailView>) {
        let Some(detail) = detail else {
            self.as_mut().set_detail(QVariant::default());
            self.as_mut().set_option_rows(QVariantList::default());
            self.as_mut().set_label_rows(QVariantList::default());
            self.as_mut().set_subnet_rows(QVariantList::default());
            self.as_mut().set_container_rows(QVariantList::default());
            return;
        };
        self.as_mut().set_detail(detail_variant(detail));
        self.as_mut()
            .set_option_rows(key_value_rows(&detail.options));
        self.as_mut().set_label_rows(key_value_rows(&detail.labels));
        self.as_mut().set_subnet_rows(subnet_rows(&detail.subnets));
        self.as_mut()
            .set_container_rows(container_rows(&detail.containers));
    }
}

#[allow(clippy::too_many_arguments)]
fn create_options(
    name: &str,
    driver: &str,
    subnet: &str,
    gateway: &str,
    enable_ipv6: bool,
    internal: bool,
    attachable: bool,
    labels_text: &str,
) -> Result<CreateNetworkOptions, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Network name is required.".into());
    }
    let subnet = optional(subnet);
    let gateway = optional(gateway);
    if gateway.is_some() && subnet.is_none() {
        return Err("A subnet is required when a gateway is specified.".into());
    }
    let mut labels = std::collections::BTreeMap::new();
    for (index, line) in labels_text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("Label on line {} has an empty key.", index + 1));
        }
        labels.insert(key.to_string(), value.to_string());
    }
    Ok(CreateNetworkOptions {
        name: name.into(),
        driver: if driver.trim().is_empty() {
            "bridge".into()
        } else {
            driver.trim().into()
        },
        subnet,
        gateway,
        ipv6: enable_ipv6,
        internal,
        attachable,
        labels,
        options: std::collections::BTreeMap::new(),
    })
}

fn detail_variant(detail: &NetworkDetailView) -> QVariant {
    let mut map = QVariantMap::default();
    for (key, value) in [
        ("networkId", detail.network_id.as_str()),
        ("shortId", detail.short_id.as_str()),
        ("name", detail.name.as_str()),
        ("createdText", detail.created_text.as_str()),
        ("createdFullText", detail.created_full_text.as_str()),
        ("driver", detail.driver.as_str()),
        ("scope", detail.scope.as_str()),
        ("subnet", detail.subnet.as_str()),
        ("gateway", detail.gateway.as_str()),
        ("ipamDriver", detail.ipam_driver.as_str()),
    ] {
        insert(&mut map, key, value);
    }
    for (key, value) in [
        ("internal", detail.internal),
        ("attachable", detail.attachable),
        ("ingress", detail.ingress),
        ("ipv4", detail.ipv4),
        ("ipv6", detail.ipv6),
    ] {
        map.insert(QString::from(key), QVariant::from(&value));
    }
    map.insert(
        QString::from("containerCount"),
        QVariant::from(&saturating_i32(detail.containers.len())),
    );
    QVariant::from(&map)
}

fn key_value_rows(rows: &[NetworkKeyValueRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "key", &row.key);
            insert(&mut map, "value", &row.value);
            QVariant::from(&map)
        })
        .collect()
}

fn subnet_rows(rows: &[NetworkSubnetView]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            insert(&mut map, "subnet", &row.subnet);
            insert(&mut map, "gateway", &row.gateway);
            insert(&mut map, "ipRange", &row.ip_range);
            map.insert(
                QString::from("auxiliaryAddresses"),
                QVariant::from(&key_value_rows(&row.auxiliary_addresses)),
            );
            QVariant::from(&map)
        })
        .collect()
}

fn container_rows(rows: &[NetworkContainerView]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            for (key, value) in [
                ("containerId", row.container_id.as_str()),
                ("shortId", row.short_id.as_str()),
                ("name", row.name.as_str()),
                ("endpointId", row.endpoint_id.as_str()),
                ("ipv4Address", row.ipv4_address.as_str()),
                ("ipv6Address", row.ipv6_address.as_str()),
                ("macAddress", row.mac_address.as_str()),
            ] {
                insert(&mut map, key, value);
            }
            QVariant::from(&map)
        })
        .collect()
}

fn safe_connection_message(status: i32, message: &str) -> String {
    match status {
        0 => "Connecting to Docker Engine…".into(),
        2 => "Docker Engine is not available. Check that Docker is running and try again.".into(),
        3 => "Permission denied while accessing Docker. Check Docker socket permissions.".into(),
        4 => "Docker could not be reached. Try again.".into(),
        _ if message.is_empty() => String::new(),
        _ => "Docker connection state changed.".into(),
    }
}

fn friendly_operation_error(error: &DockerError) -> &'static str {
    match error {
        DockerError::SocketNotFound(_)
        | DockerError::DaemonUnavailable(_)
        | DockerError::EngineUnavailable => "Docker Engine is not available.",
        DockerError::PermissionDenied => "Permission denied while accessing Docker networks.",
        DockerError::NetworkNotFound(_) => "This network no longer exists.",
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => {
            "The Docker network request timed out."
        }
        _ => "Could not load network information.",
    }
}

fn list_state_name(state: NetworksListState) -> &'static str {
    match state {
        NetworksListState::Loading => "loading",
        NetworksListState::Ready => "ready",
        NetworksListState::Empty => "empty",
        NetworksListState::Error => "error",
    }
}

fn detail_state_name(state: NetworkDetailState) -> &'static str {
    match state {
        NetworkDetailState::None => "none",
        NetworkDetailState::Loading => "loading",
        NetworkDetailState::Ready => "ready",
        NetworkDetailState::Error => "error",
    }
}

fn sort_mode_from_name(name: &str) -> Option<NetworkSortMode> {
    Some(match name {
        "name_asc" => NetworkSortMode::NameAscending,
        "name_desc" => NetworkSortMode::NameDescending,
        "newest" => NetworkSortMode::NewestFirst,
        "oldest" => NetworkSortMode::OldestFirst,
        "driver" => NetworkSortMode::Driver,
        _ => return None,
    })
}

fn sort_mode_name(mode: NetworkSortMode) -> &'static str {
    match mode {
        NetworkSortMode::NameAscending => "name_asc",
        NetworkSortMode::NameDescending => "name_desc",
        NetworkSortMode::NewestFirst => "newest",
        NetworkSortMode::OldestFirst => "oldest",
        NetworkSortMode::Driver => "driver",
    }
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

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_input_is_trimmed_and_labels_split_at_first_equals() {
        let options = create_options(
            " dev-net ",
            "",
            " 172.30.0.0/16 ",
            "172.30.0.1",
            true,
            false,
            true,
            "tier=frontend\ntoken=a=b\nflag",
        )
        .unwrap();
        assert_eq!(options.name, "dev-net");
        assert_eq!(options.driver, "bridge");
        assert_eq!(options.subnet.as_deref(), Some("172.30.0.0/16"));
        assert_eq!(options.labels["token"], "a=b");
        assert_eq!(options.labels["flag"], "");
    }

    #[test]
    fn create_input_rejects_missing_name_gateway_dependency_and_empty_label_key() {
        assert!(create_options("", "bridge", "", "", false, false, false, "").is_err());
        assert!(create_options("net", "bridge", "", "10.0.0.1", false, false, false, "").is_err());
        assert!(create_options("net", "bridge", "", "", false, false, false, "=value").is_err());
    }

    #[test]
    fn connection_messages_do_not_expose_socket_paths() {
        let message = safe_connection_message(
            2,
            "Docker socket was not found at /home/alice/private/docker.sock",
        );
        assert!(!message.contains("/home/alice"));
        assert!(message.contains("Docker Engine"));
    }

    #[test]
    fn state_and_sort_names_match_qml_contract() {
        assert_eq!(list_state_name(NetworksListState::Loading), "loading");
        assert_eq!(detail_state_name(NetworkDetailState::None), "none");
        assert_eq!(sort_mode_from_name("driver"), Some(NetworkSortMode::Driver));
        assert!(sort_mode_from_name("unknown").is_none());
    }
}
