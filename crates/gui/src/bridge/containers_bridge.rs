//! Unified Containers QAbstractListModel, controller and structured Info bridge.
//!
//! This file is intentionally self-contained so the main agent only needs to
//! register one CXX-Qt input and one module. It never calls stats while
//! refreshing summaries.

use std::collections::HashMap;
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use futures_util::{StreamExt, stream};
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    ContainerOperationState, ContainerSortMode, DockerError, GroupOperationState,
    RemoveContainerOptions, RestartContainerOptions, StopContainerOptions,
};

use crate::app_state::{get_services, get_store};
use crate::controllers::containers::{
    ContainersListState, ContainersState, GroupOperationResult, GroupTargetResult, SelectionAction,
    group_operation_name,
};
use crate::models::container_model::{
    ContainerDetailView, ContainerGroupDetailView, EnvironmentViewRow, MountViewRow,
    NetworkViewRow, PortViewRow, PropertyViewRow,
};

pub const ROLE_ROW_KIND: i32 = 257;
pub const ROLE_ID: i32 = 258;
pub const ROLE_NAME: i32 = 259;
pub const ROLE_IMAGE: i32 = 260;
pub const ROLE_STATE: i32 = 261;
pub const ROLE_STATUS: i32 = 262;
pub const ROLE_SECTION: i32 = 263;
pub const ROLE_GROUP_ID: i32 = 264;
pub const ROLE_GROUP_TOTAL_COUNT: i32 = 265;
pub const ROLE_GROUP_RUNNING_COUNT: i32 = 266;
pub const ROLE_GROUP_PAUSED_COUNT: i32 = 267;
pub const ROLE_GROUP_STOPPED_COUNT: i32 = 268;
pub const ROLE_DEPTH: i32 = 269;
pub const ROLE_EXPANDED: i32 = 270;
pub const ROLE_SELECTED: i32 = 271;
pub const ROLE_OPERATION: i32 = 272;
pub const ROLE_HEALTH: i32 = 273;
pub const ROLE_PORTS: i32 = 274;
pub const ROLE_PORTS_TEXT: i32 = 275;

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

#[derive(Default)]
pub struct ContainersListModelRust {
    pub(crate) state: ContainersState,
    pub(crate) docker_ready: bool,
    pub(crate) search_query: QString,
    pub(crate) sort_mode: QString,
    pub(crate) list_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) loading: bool,
    pub(crate) refreshing: bool,
    pub(crate) using_cache: bool,
    pub(crate) local_endpoint: bool,
    pub(crate) count: i32,
    pub(crate) total_count: i32,
    pub(crate) running_count: i32,
    pub(crate) paused_count: i32,
    pub(crate) stopped_count: i32,
    pub(crate) selection_kind: QString,
    pub(crate) selection_id: QString,
    pub(crate) selection_generation: i64,
    pub(crate) detail_state: QString,
    pub(crate) detail_error_kind: QString,
    pub(crate) detail_error_message: QString,
    pub(crate) detail_name: QString,
    pub(crate) detail_id: QString,
    pub(crate) detail_short_id: QString,
    pub(crate) detail_image: QString,
    pub(crate) detail_image_id: QString,
    pub(crate) detail_runtime_state: QString,
    pub(crate) detail_compose_project: QString,
    pub(crate) general_model: QVariantList,
    pub(crate) state_model: QVariantList,
    pub(crate) health_model: QVariantList,
    pub(crate) ports_model: QVariantList,
    pub(crate) mounts_model: QVariantList,
    pub(crate) networks_model: QVariantList,
    pub(crate) configuration_model: QVariantList,
    pub(crate) environment_model: QVariantList,
    pub(crate) labels_model: QVariantList,
    pub(crate) group_project_name: QString,
    pub(crate) group_status: QString,
    pub(crate) group_working_directory: QString,
    pub(crate) group_compose_files: QString,
    pub(crate) group_compose_version: QString,
    pub(crate) group_members_model: QVariantList,
    pub(crate) group_metadata_model: QVariantList,
    pub(crate) operation_in_progress: bool,
    pub(crate) operation_error_message: QString,
    pub(crate) last_group_result_message: QString,
    pub(crate) creating: bool,
    pub(crate) create_error_message: QString,
    pub(crate) label_search: String,
    pub(crate) label_descending: bool,
    pub(crate) refresh_cancel: Option<CancellationToken>,
    pub(crate) detail_cancel: Option<CancellationToken>,
    pub(crate) operation_cancels: HashMap<String, CancellationToken>,
    pub(crate) group_cancels: HashMap<String, CancellationToken>,
}

impl Drop for ContainersListModelRust {
    fn drop(&mut self) {
        cancel_all(self);
    }
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QAbstractListModel>);
        type QAbstractListModel;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/core/qlist/qlist_QVariant.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/core/qlist/qlist_i32.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }

    impl cxx_qt::Threading for ContainersListModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_query, cxx_name = "searchQuery")]
        #[qproperty(QString, sort_mode, cxx_name = "sortMode")]
        #[qproperty(QString, list_state, cxx_name = "listState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(bool, loading)]
        #[qproperty(bool, refreshing)]
        #[qproperty(bool, using_cache, cxx_name = "usingCache")]
        #[qproperty(bool, local_endpoint, cxx_name = "localEndpoint")]
        #[qproperty(i32, count)]
        #[qproperty(i32, total_count, cxx_name = "totalCount")]
        #[qproperty(i32, running_count, cxx_name = "runningCount")]
        #[qproperty(i32, paused_count, cxx_name = "pausedCount")]
        #[qproperty(i32, stopped_count, cxx_name = "stoppedCount")]
        #[qproperty(QString, selection_kind, cxx_name = "selectionKind")]
        #[qproperty(QString, selection_id, cxx_name = "selectionId")]
        #[qproperty(i64, selection_generation, cxx_name = "selectionGeneration")]
        #[qproperty(QString, detail_state, cxx_name = "detailState")]
        #[qproperty(QString, detail_error_kind, cxx_name = "detailErrorKind")]
        #[qproperty(QString, detail_error_message, cxx_name = "detailErrorMessage")]
        #[qproperty(QString, detail_name, cxx_name = "detailName")]
        #[qproperty(QString, detail_id, cxx_name = "detailId")]
        #[qproperty(QString, detail_short_id, cxx_name = "detailShortId")]
        #[qproperty(QString, detail_image, cxx_name = "detailImage")]
        #[qproperty(QString, detail_image_id, cxx_name = "detailImageId")]
        #[qproperty(QString, detail_runtime_state, cxx_name = "detailRuntimeState")]
        #[qproperty(QString, detail_compose_project, cxx_name = "detailComposeProject")]
        #[qproperty(QList_QVariant, general_model, cxx_name = "generalModel")]
        #[qproperty(QList_QVariant, state_model, cxx_name = "stateModel")]
        #[qproperty(QList_QVariant, health_model, cxx_name = "healthModel")]
        #[qproperty(QList_QVariant, ports_model, cxx_name = "portsModel")]
        #[qproperty(QList_QVariant, mounts_model, cxx_name = "mountsModel")]
        #[qproperty(QList_QVariant, networks_model, cxx_name = "networksModel")]
        #[qproperty(QList_QVariant, configuration_model, cxx_name = "configurationModel")]
        #[qproperty(QList_QVariant, environment_model, cxx_name = "environmentModel")]
        #[qproperty(QList_QVariant, labels_model, cxx_name = "labelsModel")]
        #[qproperty(QString, group_project_name, cxx_name = "groupProjectName")]
        #[qproperty(QString, group_status, cxx_name = "groupStatus")]
        #[qproperty(QString, group_working_directory, cxx_name = "groupWorkingDirectory")]
        #[qproperty(QString, group_compose_files, cxx_name = "groupComposeFiles")]
        #[qproperty(QString, group_compose_version, cxx_name = "groupComposeVersion")]
        #[qproperty(QList_QVariant, group_members_model, cxx_name = "groupMembersModel")]
        #[qproperty(QList_QVariant, group_metadata_model, cxx_name = "groupMetadataModel")]
        #[qproperty(bool, operation_in_progress, cxx_name = "operationInProgress")]
        #[qproperty(QString, operation_error_message, cxx_name = "operationErrorMessage")]
        #[qproperty(bool, creating)]
        #[qproperty(QString, create_error_message, cxx_name = "createErrorMessage")]
        #[qproperty(
            QString,
            last_group_result_message,
            cxx_name = "lastGroupResultMessage"
        )]
        type ContainersListModel = super::ContainersListModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;
        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;
        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);
        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);
        #[inherit]
        #[rust_name = "data_changed"]
        fn dataChanged(
            self: Pin<&mut Self>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );
        #[inherit]
        #[rust_name = "model_index"]
        fn index(self: Pin<&mut Self>, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;

        #[qsignal]
        #[cxx_name = "operationFinished"]
        fn operation_finished(
            self: Pin<&mut Self>,
            operation: QString,
            id: QString,
            success: bool,
            message: QString,
        );
        #[qsignal]
        #[cxx_name = "containerCreated"]
        fn container_created(
            self: Pin<&mut Self>,
            container_id: QString,
            started: bool,
            message: QString,
        );
        #[qsignal]
        #[cxx_name = "browserUrlRequested"]
        fn browser_url_requested(self: Pin<&mut Self>, url: QString);
        #[qsignal]
        #[cxx_name = "volumeNavigationRequested"]
        fn volume_navigation_requested(self: Pin<&mut Self>, volume_name: QString);
        #[qsignal]
        #[cxx_name = "networkNavigationRequested"]
        fn network_navigation_requested(
            self: Pin<&mut Self>,
            network_id: QString,
            network_name: QString,
        );
        #[qsignal]
        #[cxx_name = "hostPathRequested"]
        fn host_path_requested(self: Pin<&mut Self>, path: QString);
        #[qsignal]
        #[cxx_name = "removeContainerPrepared"]
        fn remove_container_prepared(
            self: Pin<&mut Self>,
            id: QString,
            name: QString,
            image: QString,
            state: QString,
            compose_project: QString,
            mounts: QList_QVariant,
        );
        #[qsignal]
        #[cxx_name = "removeContainerPreparationFailed"]
        fn remove_container_preparation_failed(self: Pin<&mut Self>, id: QString, message: QString);
        #[qsignal]
        #[cxx_name = "removeGroupPrepared"]
        fn remove_group_prepared(
            self: Pin<&mut Self>,
            id: QString,
            project_name: QString,
            targets: QList_QVariant,
        );

        #[qinvokable]
        fn initialize(self: Pin<&mut Self>);
        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "selectRow"]
        fn select_row(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "selectContainer"]
        fn select_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "toggleGroup"]
        fn toggle_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut Self>, mode: &QString);
        #[qinvokable]
        #[cxx_name = "reloadDetail"]
        fn reload_detail(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "createContainer"]
        fn create_container(self: Pin<&mut Self>, request_json: &QString);

        #[qinvokable]
        #[cxx_name = "startContainer"]
        fn start_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "stopContainer"]
        fn stop_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "restartContainer"]
        fn restart_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "killContainer"]
        fn kill_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "pauseContainer"]
        fn pause_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "unpauseContainer"]
        fn unpause_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "prepareRemoveContainer"]
        fn prepare_remove_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "removeContainer"]
        fn remove_container(
            self: Pin<&mut Self>,
            id: &QString,
            force: bool,
            remove_anonymous_volumes: bool,
        );
        #[qinvokable]
        #[cxx_name = "renameContainer"]
        fn rename_container(self: Pin<&mut Self>, id: &QString, new_name: &QString);

        #[qinvokable]
        #[cxx_name = "startGroup"]
        fn start_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "stopGroup"]
        fn stop_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "restartGroup"]
        fn restart_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "pauseGroup"]
        fn pause_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "unpauseGroup"]
        fn unpause_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "prepareRemoveGroup"]
        fn prepare_remove_group(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "removeGroup"]
        fn remove_group(
            self: Pin<&mut Self>,
            id: &QString,
            force: bool,
            remove_anonymous_volumes: bool,
        );

        #[qinvokable]
        #[cxx_name = "revealEnvironment"]
        fn reveal_environment(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "concealEnvironment"]
        fn conceal_environment(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "setLabelSearch"]
        fn set_label_search(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        #[cxx_name = "setLabelSortAscending"]
        fn set_label_sort_ascending(self: Pin<&mut Self>, ascending: bool);
        #[qinvokable]
        #[cxx_name = "requestBrowserUrl"]
        fn request_browser_url(self: Pin<&mut Self>, url: &QString);
        #[qinvokable]
        #[cxx_name = "requestVolumeNavigation"]
        fn request_volume_navigation(self: Pin<&mut Self>, volume_name: &QString);
        #[qinvokable]
        #[cxx_name = "requestNetworkNavigation"]
        fn request_network_navigation(
            self: Pin<&mut Self>,
            network_id: &QString,
            network_name: &QString,
        );
        #[qinvokable]
        #[cxx_name = "requestHostPath"]
        fn request_host_path(self: Pin<&mut Self>, path: &QString);
    }
}

impl qobject::ContainersListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.state.visible_rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.state.visible_rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            ROLE_ROW_KIND => qv(row.row_kind.as_str()),
            ROLE_ID => qv(&row.id),
            ROLE_NAME => qv(&row.name),
            ROLE_IMAGE => qv(&row.image),
            ROLE_STATE => qv(&row.state),
            ROLE_STATUS => qv(&row.status),
            ROLE_SECTION => qv(row.section.as_str()),
            ROLE_GROUP_ID => qv(&row.group_id),
            ROLE_GROUP_TOTAL_COUNT => QVariant::from(&saturating_i32(row.group_total_count)),
            ROLE_GROUP_RUNNING_COUNT => QVariant::from(&saturating_i32(row.group_running_count)),
            ROLE_GROUP_PAUSED_COUNT => QVariant::from(&saturating_i32(row.group_paused_count)),
            ROLE_GROUP_STOPPED_COUNT => QVariant::from(&saturating_i32(row.group_stopped_count)),
            ROLE_DEPTH => QVariant::from(&(row.depth as i32)),
            ROLE_EXPANDED => QVariant::from(&row.expanded),
            ROLE_SELECTED => QVariant::from(&row.selected),
            ROLE_OPERATION => qv(&row.operation),
            ROLE_HEALTH => qv(&row.health),
            ROLE_PORTS => self
                .state
                .source_rows
                .iter()
                .find(|summary| summary.id == row.id)
                .map(|summary| {
                    QVariant::from(&summary_port_rows(summary, &self.state.endpoint_key))
                })
                .unwrap_or_default(),
            ROLE_PORTS_TEXT => qv(&row.ports),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in role_pairs() {
            roles.insert(role, name.into());
        }
        roles
    }

    pub fn initialize(mut self: Pin<&mut Self>) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if !state.initialize() {
            return;
        }
        self.as_mut().apply_state(state);
        if get_services().is_some() {
            self.as_mut().rust_mut().docker_ready = true;
            self.as_mut().refresh();
        }
    }

    pub fn refresh(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().refresh_cancel.take() {
            cancel.cancel();
        }
        let store = get_store();
        let endpoint = store.endpoint.clone();
        let Some(services) = get_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.begin_refresh();
            state.apply_list_error(generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        let endpoint_key = services
            .volumes
            .client_fingerprint()
            .unwrap_or_else(|| "local".to_string());
        self.as_mut()
            .set_local_endpoint(services.client().is_local());
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.set_endpoint_key(&endpoint_key);
            let generation = state.begin_refresh();
            self.as_mut().apply_state(state);
            generation
        };
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().refresh_cancel = Some(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let mut cached_ids = Vec::new();
            if let (Some(cache), Some(endpoint)) = (store.persistent.clone(), endpoint.clone()) {
                let cached = tokio::task::spawn_blocking(move || {
                    cache.hydrate::<tuxstack_docker_core::ContainerSummary>(
                        "container_summaries",
                        &endpoint,
                    )
                })
                .await
                .unwrap_or_default();
                cached_ids = cached.iter().map(|(id, _)| id.clone()).collect();
                let cached = cached
                    .into_iter()
                    .map(|(_, summary)| summary)
                    .collect::<Vec<_>>();
                if !cached.is_empty() && !cancel.is_cancelled() {
                    let thread = qt_thread.clone();
                    if thread
                        .queue(move |mut model| {
                            let mut state = model.as_mut().rust_mut().state.clone();
                            if state.apply_cached(generation, &cached) {
                                model.as_mut().apply_state(state);
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            let options = tuxstack_docker_core::services::containers::ListContainersOptions {
                all: true,
                ..Default::default()
            };
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.containers.list_containers(&options) => result,
            };
            if let (Ok(summaries), Some(cache), Some(endpoint)) =
                (&result, store.persistent.as_ref(), endpoint.as_ref())
            {
                let live_ids = summaries
                    .iter()
                    .map(|summary| summary.id.as_str())
                    .collect::<std::collections::HashSet<_>>();
                for stale_id in cached_ids
                    .iter()
                    .filter(|id| !live_ids.contains(id.as_str()))
                {
                    cache.remove_key("container_summaries", endpoint, stale_id);
                }
                for summary in summaries {
                    cache.put("container_summaries", endpoint, &summary.id, summary);
                }
            }
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match result {
                        Ok(summaries) => state.apply_live(generation, &summaries),
                        Err(error) => state.apply_list_error(generation, &error),
                    };
                    if applied {
                        model.as_mut().rust_mut().refresh_cancel = None;
                        model.as_mut().apply_state(state);
                    }
                })
                .ok();
        });
    }

    pub fn select_row(mut self: Pin<&mut Self>, id: &QString) {
        let action = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let action = state.select_row(&id.to_string());
            self.as_mut().apply_state(state);
            action
        };
        self.as_mut().handle_selection_action(action);
    }

    pub fn select_container(mut self: Pin<&mut Self>, id: &QString) {
        let action = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let action = state.select_container(&id.to_string());
            self.as_mut().apply_state(state);
            action
        };
        self.as_mut().handle_selection_action(action);
    }

    pub fn toggle_group(mut self: Pin<&mut Self>, id: &QString) {
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.toggle_group(&id.to_string()) {
            self.as_mut().apply_state(state);
        }
    }

    pub fn set_search(mut self: Pin<&mut Self>, query: &QString) {
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_search(&query.to_string());
        self.as_mut().apply_state(state);
    }

    pub fn set_sort(mut self: Pin<&mut Self>, mode: &QString) {
        let Some(mode) = sort_mode_from_name(&mode.to_string()) else {
            return;
        };
        let mut state = self.as_mut().rust_mut().state.clone();
        state.set_sort(mode);
        self.as_mut().apply_state(state);
    }

    pub fn reload_detail(mut self: Pin<&mut Self>) {
        let action = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let action = state.reload_detail();
            self.as_mut().apply_state(state);
            action
        };
        self.as_mut().handle_selection_action(action);
    }

    pub fn set_connection_state(mut self: Pin<&mut Self>, docker_status: i32, message: &QString) {
        if !self.state.initialized {
            return;
        }
        if docker_status == 1 {
            if !self.docker_ready {
                self.as_mut().rust_mut().docker_ready = true;
                self.as_mut().refresh();
            }
            return;
        }
        self.as_mut().rust_mut().docker_ready = false;
        self.as_mut().set_local_endpoint(false);
        cancel_all(self.as_mut().rust_mut().get_mut());
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        if !state.clear_selection() {
            state.selection_generation = state.selection_generation.wrapping_add(1);
        }
        state.operations.clear();
        state.group_operations.clear();
        state.refresh_in_progress = false;
        state.list_state = match docker_status {
            0 => ContainersListState::Loading,
            3 => ContainersListState::PermissionDenied,
            2 => ContainersListState::DockerUnavailable,
            _ => ContainersListState::Error,
        };
        state.list_error_kind = match docker_status {
            3 => "permission_denied",
            2 => "docker_unavailable",
            _ => "docker",
        }
        .into();
        state.list_error_message = message.to_string();
        self.as_mut().apply_state(state);
    }

    pub fn shutdown(mut self: Pin<&mut Self>) {
        cancel_all(self.as_mut().rust_mut().get_mut());
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.selection_generation = state.selection_generation.wrapping_add(1);
        state.operation_generation = state.operation_generation.wrapping_add(1);
        state.operations.clear();
        state.group_operations.clear();
        state.refresh_in_progress = false;
        self.as_mut().apply_state(state);
    }

    pub fn create_container(mut self: Pin<&mut Self>, request_json: &QString) {
        if self.creating {
            return;
        }
        let request =
            match parse_create_request(&request_json.to_string()) {
                Ok(request) => request,
                Err(error) => {
                    self.as_mut().set_create_error_message(QString::from(format!(
                        "Invalid create request: {error}"
                    )));
                    return;
                }
            };
        if let Err(error) = request.validate() {
            self.as_mut()
                .set_create_error_message(QString::from(error.to_string()));
            return;
        }
        let Some(services) = get_services() else {
            self.as_mut()
                .set_create_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        self.as_mut().set_creating(true);
        self.as_mut().set_create_error_message(QString::default());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.containers.create_container(&request).await;
            qt_thread
                .queue(move |mut model| {
                    model.as_mut().set_creating(false);
                    match result {
                        Ok(result) => {
                            let mut messages = result.warnings;
                            messages.extend(result.network_failures.iter().map(|failure| {
                                format!("Network {}: {}", failure.network, failure.error)
                            }));
                            if let Some(error) = &result.start_error {
                                messages.push(format!("Container was created but did not start: {error}"));
                            }
                            let message = if messages.is_empty() {
                                if result.started {
                                    "Container created and started.".to_string()
                                } else {
                                    "Container created.".to_string()
                                }
                            } else {
                                messages.join("\n")
                            };
                            model.as_mut().container_created(
                                QString::from(&result.id),
                                result.started,
                                QString::from(message),
                            );
                            model.as_mut().refresh();
                        }
                        Err(error) => {
                            model
                                .as_mut()
                                .set_create_error_message(QString::from(operation_error_message(&error)));
                        }
                    }
                })
                .ok();
        });
    }

    pub fn start_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Start);
    }
    pub fn stop_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Stop);
    }
    pub fn restart_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Restart);
    }
    pub fn kill_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Kill);
    }
    pub fn pause_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Pause);
    }
    pub fn unpause_container(self: Pin<&mut Self>, id: &QString) {
        self.run_container_action(id, BridgeAction::Unpause);
    }
    pub fn prepare_remove_container(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        if id.is_empty() {
            return;
        }
        let Some(services) = get_services() else {
            self.as_mut().remove_container_preparation_failed(
                QString::from(&id),
                QString::from("Docker Engine is unavailable."),
            );
            return;
        };
        let cancel_key = format!("prepare-remove:{id}");
        let cancel = CancellationToken::new();
        if let Some(previous) = self
            .as_mut()
            .rust_mut()
            .operation_cancels
            .insert(cancel_key.clone(), cancel.clone())
        {
            previous.cancel();
        }
        let endpoint_key = self.state.endpoint_key.clone();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.containers.inspect_container(&id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    model
                        .as_mut()
                        .rust_mut()
                        .operation_cancels
                        .remove(&cancel_key);
                    match result {
                        Ok(detail) => {
                            let view = ContainerDetailView::from_detail_for_endpoint(
                                &detail,
                                &endpoint_key,
                            );
                            model.as_mut().remove_container_prepared(
                                QString::from(&view.id),
                                QString::from(&view.name),
                                QString::from(&view.image),
                                QString::from(&view.state_name),
                                QString::from(&view.compose_project),
                                mount_rows(&view.mounts),
                            );
                        }
                        Err(error) => model.as_mut().remove_container_preparation_failed(
                            QString::from(&id),
                            QString::from(operation_error_message(&error)),
                        ),
                    }
                })
                .ok();
        });
    }

    pub fn remove_container(
        self: Pin<&mut Self>,
        id: &QString,
        force: bool,
        remove_anonymous_volumes: bool,
    ) {
        self.run_container_action(
            id,
            BridgeAction::Remove {
                force,
                remove_volumes: remove_anonymous_volumes,
            },
        );
    }
    pub fn rename_container(self: Pin<&mut Self>, id: &QString, new_name: &QString) {
        self.run_container_action(
            id,
            BridgeAction::Rename {
                new_name: new_name.to_string(),
            },
        );
    }

    pub fn start_group(self: Pin<&mut Self>, id: &QString) {
        self.run_group_action(id, GroupBridgeAction::Start);
    }
    pub fn stop_group(self: Pin<&mut Self>, id: &QString) {
        self.run_group_action(id, GroupBridgeAction::Stop);
    }
    pub fn restart_group(self: Pin<&mut Self>, id: &QString) {
        self.run_group_action(id, GroupBridgeAction::Restart);
    }
    pub fn pause_group(self: Pin<&mut Self>, id: &QString) {
        self.run_group_action(id, GroupBridgeAction::Pause);
    }
    pub fn unpause_group(self: Pin<&mut Self>, id: &QString) {
        self.run_group_action(id, GroupBridgeAction::Unpause);
    }
    pub fn prepare_remove_group(mut self: Pin<&mut Self>, id: &QString) {
        let id_string = id.to_string();
        let Some(group_id) = self.state.group_id_from_opaque(&id_string) else {
            return;
        };
        let Some(group) = self.state.groups.iter().find(|group| group.id == group_id) else {
            return;
        };
        let targets = group
            .containers
            .iter()
            .filter_map(|id| self.state.source_rows.iter().find(|row| &row.id == id))
            .map(|row| {
                variant_map(&[
                    ("id", row.id.as_str()),
                    ("name", row.name.as_str()),
                    ("image", row.image.as_str()),
                    ("state", row.state.as_str()),
                ])
            })
            .collect();
        let project_name = group.project_name.clone();
        self.as_mut().remove_group_prepared(
            QString::from(id_string),
            QString::from(project_name),
            targets,
        );
    }

    pub fn remove_group(
        self: Pin<&mut Self>,
        id: &QString,
        force: bool,
        remove_anonymous_volumes: bool,
    ) {
        self.run_group_action(
            id,
            GroupBridgeAction::Remove {
                force,
                remove_volumes: remove_anonymous_volumes,
            },
        );
    }

    pub fn reveal_environment(mut self: Pin<&mut Self>, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.reveal_environment(index) {
            self.as_mut().apply_state(state);
        }
    }

    pub fn conceal_environment(mut self: Pin<&mut Self>, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let mut state = self.as_mut().rust_mut().state.clone();
        if state.conceal_environment(index) {
            self.as_mut().apply_state(state);
        }
    }

    pub fn set_label_search(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut().rust_mut().label_search = query.to_string().trim().to_ascii_lowercase();
        let detail = self.state.container_detail.clone();
        self.as_mut().sync_labels(detail.as_ref());
    }

    pub fn set_label_sort_ascending(mut self: Pin<&mut Self>, ascending: bool) {
        self.as_mut().rust_mut().label_descending = !ascending;
        let detail = self.state.container_detail.clone();
        self.as_mut().sync_labels(detail.as_ref());
    }

    pub fn request_browser_url(mut self: Pin<&mut Self>, url: &QString) {
        let url = url.to_string();
        if matches!(url.split_once("://"), Some(("http" | "https", _))) {
            self.as_mut().browser_url_requested(QString::from(url));
        }
    }

    pub fn request_volume_navigation(mut self: Pin<&mut Self>, volume_name: &QString) {
        if !volume_name.is_empty() {
            self.as_mut()
                .volume_navigation_requested(volume_name.clone());
        }
    }

    pub fn request_network_navigation(
        mut self: Pin<&mut Self>,
        network_id: &QString,
        network_name: &QString,
    ) {
        if !network_id.is_empty() || !network_name.is_empty() {
            self.as_mut()
                .network_navigation_requested(network_id.clone(), network_name.clone());
        }
    }

    pub fn request_host_path(mut self: Pin<&mut Self>, path: &QString) {
        if path.is_empty() || !self.local_endpoint {
            return;
        }
        let path_string = path.to_string();
        let path = std::path::Path::new(&path_string);
        if path.is_dir() {
            self.as_mut()
                .host_path_requested(QString::from(path_string));
        }
    }

    fn handle_selection_action(mut self: Pin<&mut Self>, action: SelectionAction) {
        if let Some(cancel) = self.as_mut().rust_mut().detail_cancel.take() {
            cancel.cancel();
        }
        let SelectionAction::LoadContainer { generation } = action else {
            return;
        };
        let container_id = self.state.selection_id();
        let Some(services) = get_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_detail_error(generation, &DockerError::EngineUnavailable);
            self.as_mut().apply_state(state);
            return;
        };
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().detail_cancel = Some(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.containers.inspect_container(&container_id) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    let applied = match result {
                        Ok(detail) => state.apply_detail(generation, &detail),
                        Err(error) => state.apply_detail_error(generation, &error),
                    };
                    if applied {
                        model.as_mut().rust_mut().detail_cancel = None;
                        model.as_mut().apply_state(state);
                    }
                })
                .ok();
        });
    }

    fn run_container_action(mut self: Pin<&mut Self>, id: &QString, action: BridgeAction) {
        let id = id.to_string();
        let operation = action.operation();
        let Some(services) = get_services() else {
            self.as_mut()
                .set_operation_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let generation = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(generation) = state.begin_operation(&id, operation) else {
                return;
            };
            self.as_mut()
                .set_operation_error_message(QString::default());
            self.as_mut().apply_state(state);
            generation
        };
        let cancel = CancellationToken::new();
        self.as_mut()
            .rust_mut()
            .operation_cancels
            .insert(id.clone(), cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => Err(DockerError::OperationCancelled),
                result = execute_container_action(&services, &id, &action) => result,
            };
            qt_thread
                .queue(move |mut model| {
                    let mut state = model.as_mut().rust_mut().state.clone();
                    model.as_mut().rust_mut().operation_cancels.remove(&id);
                    let success = result.is_ok();
                    let mut selection_action = SelectionAction::None;
                    let message = match result {
                        Ok(()) => {
                            state.finish_operation(&id, generation, operation);
                            if operation == ContainerOperationState::Removing {
                                selection_action =
                                    state.remove_local_many(std::slice::from_ref(&id));
                            }
                            success_message(&action, &id)
                        }
                        Err(error) => {
                            state.fail_operation(&id, generation, operation, &error);
                            operation_error_message(&error)
                        }
                    };
                    model.as_mut().apply_state(state);
                    model.as_mut().handle_selection_action(selection_action);
                    if !success {
                        model
                            .as_mut()
                            .set_operation_error_message(QString::from(&message));
                    }
                    model.as_mut().operation_finished(
                        QString::from(action.name()),
                        QString::from(&id),
                        success,
                        QString::from(message),
                    );
                    if success {
                        model.as_mut().refresh();
                    }
                })
                .ok();
        });
    }

    fn run_group_action(mut self: Pin<&mut Self>, id: &QString, action: GroupBridgeAction) {
        let id = id.to_string();
        let operation = action.operation();
        let Some(services) = get_services() else {
            self.as_mut()
                .set_operation_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let (group_id, generation, targets) = {
            let mut state = self.as_mut().rust_mut().state.clone();
            let Some(group_id) = state.group_id_from_opaque(&id) else {
                return;
            };
            let Some((generation, targets)) = state.begin_group_operation(&id, operation) else {
                return;
            };
            self.as_mut().apply_state(state);
            (group_id, generation, targets)
        };
        let names = self
            .state
            .source_rows
            .iter()
            .map(|row| (row.id.clone(), row.name.clone()))
            .collect::<HashMap<_, _>>();
        let cancel = CancellationToken::new();
        self.as_mut()
            .rust_mut()
            .group_cancels
            .insert(id.clone(), cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let results = if matches!(
                &action,
                GroupBridgeAction::Remove { force: true, .. }
                    | GroupBridgeAction::Remove {
                        remove_volumes: true,
                        ..
                    }
            ) {
                // The current Compose API deliberately exposes safe removal
                // only. Explicit force/anonymous-volume options therefore use
                // the documented real-container fallback, bounded to six.
                run_group_container_fallback(
                    services.clone(),
                    targets.clone(),
                    names.clone(),
                    action.clone(),
                    cancel.clone(),
                )
                .await
            } else {
                let official = tokio::select! {
                    _ = cancel.cancelled() => return,
                    result = execute_compose_action(&services, &group_id, &action) => result,
                };
                match official {
                    Ok(result) => result
                        .members
                        .into_iter()
                        .map(|member| GroupTargetResult {
                            container_name: names
                                .get(&member.container_id)
                                .cloned()
                                .unwrap_or_else(|| member.container_id.clone()),
                            success: member.error.is_none(),
                            error: member.error.unwrap_or_default(),
                            container_id: member.container_id,
                        })
                        .collect(),
                    Err(error) => targets
                        .into_iter()
                        .map(|container_id| GroupTargetResult {
                            container_name: names
                                .get(&container_id)
                                .cloned()
                                .unwrap_or_else(|| container_id.clone()),
                            container_id,
                            success: false,
                            error: operation_error_message(&error),
                        })
                        .collect(),
                }
            };
            qt_thread
                .queue(move |mut model| {
                    model.as_mut().rust_mut().group_cancels.remove(&id);
                    let removed_ids = if operation == GroupOperationState::Removing {
                        results
                            .iter()
                            .filter(|result| result.success)
                            .map(|result| result.container_id.clone())
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    let result = GroupOperationResult {
                        operation,
                        targets: results,
                    };
                    let success = result.failure_count() == 0;
                    let message = result.message();
                    let mut state = model.as_mut().rust_mut().state.clone();
                    state.finish_group_operation(&id, generation, result);
                    let selection_action = if !removed_ids.is_empty() {
                        state.remove_local_many(&removed_ids)
                    } else {
                        SelectionAction::None
                    };
                    model.as_mut().apply_state(state);
                    model.as_mut().handle_selection_action(selection_action);
                    model.as_mut().operation_finished(
                        QString::from(group_operation_name(operation)),
                        QString::from(&id),
                        success,
                        QString::from(&message),
                    );
                    model.as_mut().refresh();
                })
                .ok();
        });
    }

    fn apply_state(mut self: Pin<&mut Self>, state: ContainersState) {
        let topology_unchanged = self.state.visible_rows.len() == state.visible_rows.len()
            && self
                .state
                .visible_rows
                .iter()
                .zip(&state.visible_rows)
                .all(|(old, new)| old.row_kind == new.row_kind && old.id == new.id);
        if topology_unchanged {
            self.as_mut().rust_mut().state = state;
            let row_count = self.state.visible_rows.len();
            if row_count > 0 {
                let parent = QModelIndex::default();
                let top = self.as_mut().model_index(0, 0, &parent);
                let bottom = self
                    .as_mut()
                    .model_index((row_count - 1) as i32, 0, &parent);
                let roles = cxx_qt_lib::QList::<i32>::default();
                self.as_mut().data_changed(&top, &bottom, &roles);
            }
        } else {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().state = state;
            self.as_mut().end_reset_model();
        }
        let state = self.state.clone();
        self.as_mut()
            .set_search_query(QString::from(&state.search_query));
        self.as_mut()
            .set_sort_mode(QString::from(sort_mode_name(state.sort_mode)));
        self.as_mut()
            .set_list_state(QString::from(state.list_state.as_str()));
        self.as_mut()
            .set_error_kind(QString::from(&state.list_error_kind));
        self.as_mut()
            .set_error_message(QString::from(&state.list_error_message));
        self.as_mut()
            .set_loading(state.list_state == ContainersListState::Loading);
        self.as_mut().set_refreshing(state.refresh_in_progress);
        self.as_mut().set_using_cache(state.using_cache);
        self.as_mut()
            .set_count(saturating_i32(state.visible_rows.len()));
        self.as_mut()
            .set_total_count(saturating_i32(state.total_count()));
        self.as_mut()
            .set_running_count(saturating_i32(state.running_count()));
        self.as_mut()
            .set_paused_count(saturating_i32(state.paused_count()));
        self.as_mut()
            .set_stopped_count(saturating_i32(state.stopped_count()));
        self.as_mut()
            .set_selection_kind(QString::from(state.selection_kind()));
        self.as_mut()
            .set_selection_id(QString::from(state.selection_id()));
        self.as_mut()
            .set_selection_generation(saturating_i64(state.selection_generation));
        self.as_mut()
            .set_detail_state(QString::from(state.detail_state.as_str()));
        self.as_mut()
            .set_detail_error_kind(QString::from(&state.detail_error_kind));
        self.as_mut()
            .set_detail_error_message(QString::from(&state.detail_error_message));
        self.as_mut().set_operation_in_progress(
            !state.operations.is_empty() || !state.group_operations.is_empty(),
        );
        self.as_mut().set_last_group_result_message(QString::from(
            state
                .last_group_result
                .as_ref()
                .map(GroupOperationResult::message)
                .unwrap_or_default(),
        ));
        self.as_mut()
            .sync_container_detail(state.container_detail.as_ref());
        self.as_mut().sync_group_detail(state.group_detail.as_ref());
    }

    fn sync_container_detail(mut self: Pin<&mut Self>, detail: Option<&ContainerDetailView>) {
        let empty = ContainerDetailView::default();
        let detail = detail.unwrap_or(&empty);
        self.as_mut().set_detail_name(QString::from(&detail.name));
        self.as_mut().set_detail_id(QString::from(&detail.id));
        self.as_mut()
            .set_detail_short_id(QString::from(&detail.short_id));
        self.as_mut().set_detail_image(QString::from(&detail.image));
        self.as_mut()
            .set_detail_image_id(QString::from(&detail.image_id));
        self.as_mut()
            .set_detail_runtime_state(QString::from(&detail.state_name));
        self.as_mut()
            .set_detail_compose_project(QString::from(&detail.compose_project));
        self.as_mut()
            .set_general_model(property_rows(&detail.general));
        self.as_mut().set_state_model(property_rows(&detail.state));
        self.as_mut()
            .set_health_model(property_rows(&detail.health));
        self.as_mut().set_ports_model(port_rows(&detail.ports));
        self.as_mut().set_mounts_model(mount_rows(&detail.mounts));
        self.as_mut()
            .set_networks_model(network_rows(&detail.networks));
        self.as_mut()
            .set_configuration_model(property_rows(&detail.configuration));
        self.as_mut()
            .set_environment_model(environment_rows(&detail.environment));
        self.as_mut().sync_labels(Some(detail));
    }

    fn sync_labels(mut self: Pin<&mut Self>, detail: Option<&ContainerDetailView>) {
        let search = self.label_search.clone();
        let descending = self.label_descending;
        let mut labels = detail
            .map(|detail| {
                detail
                    .labels
                    .iter()
                    .filter(|row| {
                        search.is_empty()
                            || row.key.to_ascii_lowercase().contains(&search)
                            || row.value.to_ascii_lowercase().contains(&search)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        labels.sort_by(|left, right| {
            let order = left
                .key
                .to_ascii_lowercase()
                .cmp(&right.key.to_ascii_lowercase())
                .then_with(|| left.value.cmp(&right.value));
            if descending { order.reverse() } else { order }
        });
        self.as_mut().set_labels_model(property_rows(&labels));
    }

    fn sync_group_detail(mut self: Pin<&mut Self>, detail: Option<&ContainerGroupDetailView>) {
        let empty = ContainerGroupDetailView::default();
        let detail = detail.unwrap_or(&empty);
        self.as_mut()
            .set_group_project_name(QString::from(&detail.project_name));
        self.as_mut()
            .set_group_status(QString::from(&detail.status));
        self.as_mut()
            .set_group_working_directory(QString::from(&detail.working_directory));
        self.as_mut()
            .set_group_compose_files(QString::from(&detail.compose_files));
        self.as_mut()
            .set_group_compose_version(QString::from(&detail.compose_version));
        self.as_mut()
            .set_group_metadata_model(property_rows(&detail.metadata));
        let members = detail
            .members
            .iter()
            .map(|member| {
                variant_map(&[
                    ("id", member.id.as_str()),
                    ("name", member.name.as_str()),
                    ("service", member.service.as_str()),
                    ("state", member.state.as_str()),
                    ("image", member.image.as_str()),
                ])
            })
            .collect();
        self.as_mut().set_group_members_model(members);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BridgeAction {
    Start,
    Stop,
    Restart,
    Kill,
    Pause,
    Unpause,
    Remove { force: bool, remove_volumes: bool },
    Rename { new_name: String },
}

impl BridgeAction {
    fn operation(&self) -> ContainerOperationState {
        match self {
            Self::Start => ContainerOperationState::Starting,
            Self::Stop => ContainerOperationState::Stopping,
            Self::Restart => ContainerOperationState::Restarting,
            Self::Kill => ContainerOperationState::Killing,
            Self::Pause => ContainerOperationState::Pausing,
            Self::Unpause => ContainerOperationState::Unpausing,
            Self::Remove { .. } => ContainerOperationState::Removing,
            Self::Rename { .. } => ContainerOperationState::Renaming,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Kill => "kill",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Remove { .. } => "remove",
            Self::Rename { .. } => "rename",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupBridgeAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove { force: bool, remove_volumes: bool },
}

impl GroupBridgeAction {
    fn operation(&self) -> GroupOperationState {
        match self {
            Self::Start => GroupOperationState::Starting,
            Self::Stop => GroupOperationState::Stopping,
            Self::Restart => GroupOperationState::Restarting,
            Self::Pause => GroupOperationState::Pausing,
            Self::Unpause => GroupOperationState::Unpausing,
            Self::Remove { .. } => GroupOperationState::Removing,
        }
    }

    fn container_action(&self) -> BridgeAction {
        match self {
            Self::Start => BridgeAction::Start,
            Self::Stop => BridgeAction::Stop,
            Self::Restart => BridgeAction::Restart,
            Self::Pause => BridgeAction::Pause,
            Self::Unpause => BridgeAction::Unpause,
            Self::Remove {
                force,
                remove_volumes,
            } => BridgeAction::Remove {
                force: *force,
                remove_volumes: *remove_volumes,
            },
        }
    }
}

async fn execute_compose_action(
    services: &tuxstack_docker_core::DockerServices,
    group_id: &tuxstack_docker_core::ContainerGroupId,
    action: &GroupBridgeAction,
) -> Result<tuxstack_docker_core::ContainerGroupOperationResult, DockerError> {
    match action {
        GroupBridgeAction::Start => services.compose.start_group(group_id).await,
        GroupBridgeAction::Stop => services.compose.stop_group(group_id).await,
        GroupBridgeAction::Restart => services.compose.restart_group(group_id).await,
        GroupBridgeAction::Pause => services.compose.pause_group(group_id).await,
        GroupBridgeAction::Unpause => services.compose.unpause_group(group_id).await,
        GroupBridgeAction::Remove { .. } => services.compose.remove_group(group_id).await,
    }
}

async fn run_group_container_fallback(
    services: std::sync::Arc<tuxstack_docker_core::DockerServices>,
    targets: Vec<String>,
    names: HashMap<String, String>,
    action: GroupBridgeAction,
    cancel: CancellationToken,
) -> Vec<GroupTargetResult> {
    stream::iter(targets.into_iter().map(|target| {
        let services = services.clone();
        let action = action.clone();
        let name = names
            .get(&target)
            .cloned()
            .unwrap_or_else(|| target.clone());
        async move {
            let result =
                execute_container_action(&services, &target, &action.container_action()).await;
            GroupTargetResult {
                container_id: target,
                container_name: name,
                success: result.is_ok(),
                error: result
                    .err()
                    .map(|error| operation_error_message(&error))
                    .unwrap_or_default(),
            }
        }
    }))
    .buffer_unordered(6)
    .take_until(cancel.cancelled_owned())
    .collect::<Vec<_>>()
    .await
}

async fn execute_container_action(
    services: &tuxstack_docker_core::DockerServices,
    id: &str,
    action: &BridgeAction,
) -> Result<(), DockerError> {
    match action {
        BridgeAction::Start => services.containers.start_container(id).await,
        BridgeAction::Stop => {
            services
                .containers
                .stop_container(
                    id,
                    Some(&StopContainerOptions {
                        timeout_seconds: Some(10),
                    }),
                )
                .await
        }
        BridgeAction::Restart => {
            services
                .containers
                .restart_container_with_options(
                    id,
                    &RestartContainerOptions {
                        timeout_seconds: Some(10),
                    },
                )
                .await
        }
        BridgeAction::Kill => services.containers.kill_container(id).await,
        BridgeAction::Pause => services.containers.pause_container(id).await,
        BridgeAction::Unpause => services.containers.unpause_container(id).await,
        BridgeAction::Remove {
            force,
            remove_volumes,
        } => {
            services
                .containers
                .remove_container(
                    id,
                    &RemoveContainerOptions {
                        force: *force,
                        remove_volumes: *remove_volumes,
                        remove_links: false,
                    },
                )
                .await
        }
        BridgeAction::Rename { new_name } => {
            services.containers.rename_container(id, new_name).await
        }
    }
}

fn success_message(action: &BridgeAction, id: &str) -> String {
    match action {
        BridgeAction::Rename { new_name } => format!("Renamed {id} to {new_name}."),
        _ => format!("Container {id}: {} completed.", action.name()),
    }
}

fn operation_error_message(error: &DockerError) -> String {
    match error {
        DockerError::ContainerNotFound(_) => "The container no longer exists.".into(),
        DockerError::PermissionDenied => "Permission denied while accessing Docker.".into(),
        DockerError::EngineUnavailable | DockerError::SocketNotFound(_) => {
            "Docker Engine is unavailable.".into()
        }
        DockerError::OperationTimeout | DockerError::ConnectionTimeout => {
            "The Docker container operation timed out.".into()
        }
        DockerError::Conflict(message)
        | DockerError::InvalidContainerConfig(message)
        | DockerError::Api(message) => message.clone(),
        DockerError::OperationCancelled => "The container operation was cancelled.".into(),
        other => other.to_string(),
    }
}

fn role_pairs() -> [(i32, &'static str); 19] {
    [
        (ROLE_ROW_KIND, "rowKind"),
        (ROLE_ID, "id"),
        (ROLE_NAME, "name"),
        (ROLE_IMAGE, "image"),
        (ROLE_STATE, "state"),
        (ROLE_STATUS, "status"),
        (ROLE_SECTION, "section"),
        (ROLE_GROUP_ID, "groupId"),
        (ROLE_GROUP_TOTAL_COUNT, "groupTotalCount"),
        (ROLE_GROUP_RUNNING_COUNT, "groupRunningCount"),
        (ROLE_GROUP_PAUSED_COUNT, "groupPausedCount"),
        (ROLE_GROUP_STOPPED_COUNT, "groupStoppedCount"),
        (ROLE_DEPTH, "depth"),
        (ROLE_EXPANDED, "expanded"),
        (ROLE_SELECTED, "selected"),
        (ROLE_OPERATION, "operation"),
        (ROLE_HEALTH, "health"),
        (ROLE_PORTS, "ports"),
        (ROLE_PORTS_TEXT, "portsText"),
    ]
}

fn sort_mode_from_name(name: &str) -> Option<ContainerSortMode> {
    Some(match name {
        "name_asc" => ContainerSortMode::NameAscending,
        "name_desc" => ContainerSortMode::NameDescending,
        "newest" => ContainerSortMode::NewestFirst,
        "oldest" => ContainerSortMode::OldestFirst,
        "running_first" => ContainerSortMode::RunningFirst,
        "stopped_first" => ContainerSortMode::StoppedFirst,
        "groups_first" => ContainerSortMode::ComposeGroupsFirst,
        "individual_first" => ContainerSortMode::IndividualContainersFirst,
        _ => return None,
    })
}

fn sort_mode_name(mode: ContainerSortMode) -> &'static str {
    match mode {
        ContainerSortMode::NameAscending => "name_asc",
        ContainerSortMode::NameDescending => "name_desc",
        ContainerSortMode::NewestFirst => "newest",
        ContainerSortMode::OldestFirst => "oldest",
        ContainerSortMode::RunningFirst => "running_first",
        ContainerSortMode::StoppedFirst => "stopped_first",
        ContainerSortMode::ComposeGroupsFirst => "groups_first",
        ContainerSortMode::IndividualContainersFirst => "individual_first",
    }
}

fn summary_port_rows(
    summary: &tuxstack_docker_core::ContainerSummary,
    endpoint_key: &str,
) -> QVariantList {
    summary
        .ports
        .iter()
        .filter(|port| port.is_published())
        .map(|port| {
            let host_ip = port.host_ip.as_deref().unwrap_or_default();
            let browser_url =
                published_browser_url(endpoint_key, host_ip, port.host_port, port.container_port);
            let mut map = QVariantMap::default();
            insert(&mut map, "containerPort", &port.container_port.to_string());
            insert(&mut map, "protocol", &port.protocol);
            insert(&mut map, "hostIp", host_ip);
            insert(
                &mut map,
                "hostPort",
                &port
                    .host_port
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            );
            insert(&mut map, "browserUrl", &browser_url);
            map.insert(
                QString::from("published"),
                QVariant::from(&port.is_published()),
            );
            QVariant::from(&map)
        })
        .collect()
}

fn published_browser_url(
    endpoint_key: &str,
    host_ip: &str,
    host_port: Option<u16>,
    container_port: u16,
) -> String {
    let Some(port) = host_port else {
        return String::new();
    };
    let wildcard = matches!(host_ip, "" | "0.0.0.0" | "::" | "[::]");
    let host = if wildcard {
        endpoint_host(endpoint_key).unwrap_or_else(|| "localhost".to_string())
    } else {
        host_ip.to_string()
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    let scheme = if port == 443 || container_port == 443 {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://{host}:{port}")
}

fn endpoint_host(endpoint_key: &str) -> Option<String> {
    if endpoint_key == "local"
        || endpoint_key == "default-local"
        || endpoint_key.starts_with("unix://")
        || endpoint_key.starts_with("npipe://")
    {
        return None;
    }
    let authority = endpoint_key
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(endpoint_key)
        .split('/')
        .next()
        .unwrap_or(endpoint_key)
        .rsplit('@')
        .next()
        .unwrap_or(endpoint_key);
    if let Some(end) = authority.find(']') {
        return Some(authority[1..end].to_string());
    }
    Some(authority.split(':').next().unwrap_or(authority).to_string())
}

fn property_rows(rows: &[PropertyViewRow]) -> QVariantList {
    rows.iter()
        .map(|row| variant_map(&[("key", &row.key), ("value", &row.value)]))
        .collect()
}

fn environment_rows(rows: &[EnvironmentViewRow]) -> QVariantList {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let mut map = QVariantMap::default();
            insert(&mut map, "key", &row.key);
            insert(&mut map, "value", row.masked_value());
            map.insert(
                QString::from("index"),
                QVariant::from(&saturating_i32(index)),
            );
            map.insert(QString::from("revealed"), QVariant::from(&row.revealed));
            QVariant::from(&map)
        })
        .collect()
}

fn port_rows(rows: &[PortViewRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            let mut map = QVariantMap::default();
            for (key, value) in [
                ("containerPort", row.container_port.as_str()),
                ("protocol", row.protocol.as_str()),
                ("hostIp", row.host_ip.as_str()),
                ("hostPort", row.host_port.as_str()),
                ("browserUrl", row.browser_url.as_str()),
            ] {
                insert(&mut map, key, value);
            }
            map.insert(QString::from("published"), QVariant::from(&row.published));
            QVariant::from(&map)
        })
        .collect()
}

fn mount_rows(rows: &[MountViewRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            variant_map(&[
                ("type", &row.mount_type),
                ("source", &row.source),
                ("destination", &row.destination),
                ("access", &row.access),
                ("propagation", &row.propagation),
                ("volumeName", &row.volume_name),
            ])
        })
        .collect()
}

fn network_rows(rows: &[NetworkViewRow]) -> QVariantList {
    rows.iter()
        .map(|row| {
            variant_map(&[
                ("name", &row.name),
                ("id", &row.id),
                ("ipv4", &row.ipv4),
                ("ipv6", &row.ipv6),
                ("gateway", &row.gateway),
                ("mac", &row.mac),
                ("aliases", &row.aliases),
                ("endpointId", &row.endpoint_id),
            ])
        })
        .collect()
}

fn variant_map(values: &[(&str, &str)]) -> QVariant {
    let mut map = QVariantMap::default();
    for (key, value) in values {
        insert(&mut map, key, value);
    }
    QVariant::from(&map)
}

fn insert(map: &mut QVariantMap, key: &str, value: &str) {
    map.insert(QString::from(key), qv(value));
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn parse_create_request(
    json: &str,
) -> Result<tuxstack_docker_core::CreateContainerRequest, serde_json::Error> {
    serde_json::from_str(json)
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn saturating_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn cancel_all(rust: &mut ContainersListModelRust) {
    if let Some(cancel) = rust.refresh_cancel.take() {
        cancel.cancel();
    }
    if let Some(cancel) = rust.detail_cancel.take() {
        cancel.cancel();
    }
    for (_, cancel) in rust.operation_cancels.drain() {
        cancel.cancel();
    }
    for (_, cancel) in rust.group_cancels.drain() {
        cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ids_are_explicit_contiguous_and_start_at_user_role_plus_one() {
        let roles = role_pairs();
        assert_eq!(roles.first(), Some(&(257, "rowKind")));
        assert_eq!(roles.last(), Some(&(275, "portsText")));
        for (offset, (role, _)) in roles.iter().enumerate() {
            assert_eq!(*role, 257 + offset as i32);
        }
    }

    #[test]
    fn all_eight_qml_sort_names_map_both_directions() {
        for name in [
            "name_asc",
            "name_desc",
            "newest",
            "oldest",
            "running_first",
            "stopped_first",
            "groups_first",
            "individual_first",
        ] {
            let mode = sort_mode_from_name(name).unwrap();
            assert_eq!(sort_mode_name(mode), name);
        }
        assert!(sort_mode_from_name("invalid").is_none());
    }

    #[test]
    fn bridge_actions_map_to_typed_busy_operations() {
        assert_eq!(
            BridgeAction::Start.operation(),
            ContainerOperationState::Starting
        );
        assert_eq!(
            BridgeAction::Stop.operation(),
            ContainerOperationState::Stopping
        );
        assert_eq!(
            BridgeAction::Restart.operation(),
            ContainerOperationState::Restarting
        );
        assert_eq!(
            BridgeAction::Kill.operation(),
            ContainerOperationState::Killing
        );
        assert_eq!(
            BridgeAction::Pause.operation(),
            ContainerOperationState::Pausing
        );
        assert_eq!(
            BridgeAction::Unpause.operation(),
            ContainerOperationState::Unpausing
        );
        assert_eq!(
            BridgeAction::Remove {
                force: false,
                remove_volumes: false
            }
            .operation(),
            ContainerOperationState::Removing
        );
        assert_eq!(
            BridgeAction::Rename {
                new_name: "x".into()
            }
            .operation(),
            ContainerOperationState::Renaming
        );
    }

    #[test]
    fn group_actions_map_to_real_container_fallback_actions() {
        assert_eq!(
            GroupBridgeAction::Start.container_action(),
            BridgeAction::Start
        );
        assert_eq!(
            GroupBridgeAction::Pause.operation(),
            GroupOperationState::Pausing
        );
        assert_eq!(
            GroupBridgeAction::Remove {
                force: true,
                remove_volumes: false
            }
            .container_action(),
            BridgeAction::Remove {
                force: true,
                remove_volumes: false
            }
        );
    }

    #[test]
    fn create_request_json_is_typed_and_environment_debug_is_redacted() {
        let request = parse_create_request(
            r#"{
                "name":"web",
                "image":"nginx:latest",
                "platform":null,
                "hostname":null,
                "domain_name":null,
                "entrypoint":["/docker-entrypoint.sh"],
                "command":["nginx","-g","daemon off;"],
                "working_directory":"/",
                "user":null,
                "tty":false,
                "open_stdin":false,
                "ports":[{"container_port":80,"protocol":"tcp","host_ip":null,"host_port":8080}],
                "mounts":[{"Volume":{"source":"data","destination":"/data","read_only":false}}],
                "environment":[{"key":"TOKEN","value":"super-secret"}],
                "networks":[],
                "resources":{"cpu_cores_millis":1000,"memory_bytes":134217728,"pids_limit":64},
                "restart_policy":{"name":"no","maximum_retry_count":null},
                "labels":{},
                "read_only_rootfs":false,
                "privileged":false,
                "auto_remove":false,
                "create_and_start":true
            }"#,
        )
        .unwrap();
        request.validate().unwrap();
        assert_eq!(request.command[2], "daemon off;");
        assert_eq!(request.ports[0].host_port, Some(8080));
        assert!(!format!("{request:?}").contains("super-secret"));
    }

    #[test]
    fn operation_error_mapping_keeps_daemon_conflict_detail() {
        let message =
            operation_error_message(&DockerError::Conflict("port already allocated".into()));
        assert_eq!(message, "port already allocated");
        assert_eq!(
            operation_error_message(&DockerError::PermissionDenied),
            "Permission denied while accessing Docker."
        );
    }
}
