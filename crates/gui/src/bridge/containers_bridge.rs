//! Unified Containers QAbstractListModel, controller and structured Info bridge.
//!
//! This file is intentionally self-contained so the main agent only needs to
//! register one CXX-Qt input and one module. It never calls stats while
//! refreshing summaries.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use futures_util::StreamExt;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::services::ComposeGroupAction;
use tuxstack_docker_core::{
    ContainerOperationState, ContainerSortMode, DockerError, GroupOperationState, PullImageOptions,
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
    pub(crate) create_generation: u64,
    pub(crate) create_cancel: Option<CancellationToken>,
    pub(crate) environment_import_generation: u64,
    pub(crate) environment_import_cancel: Option<CancellationToken>,
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
        #[cxx_name = "imagePullRequired"]
        fn image_pull_required(
            self: Pin<&mut Self>,
            request_json: QString,
            image_reference: QString,
        );
        #[qsignal]
        #[cxx_name = "environmentFileImported"]
        fn environment_file_imported(
            self: Pin<&mut Self>,
            entries: QList_QVariant,
            message: QString,
        );
        #[qsignal]
        #[cxx_name = "environmentFileImportFailed"]
        fn environment_file_import_failed(self: Pin<&mut Self>, message: QString);
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
        fn remove_container_prepared(self: Pin<&mut Self>, preparation: QVariant);
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
        #[cxx_name = "clearCreateError"]
        fn clear_create_error(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "createContainer"]
        fn create_container(self: Pin<&mut Self>, requestJson: &QString);
        #[qinvokable]
        #[cxx_name = "confirmPullAndCreate"]
        fn confirm_pull_and_create(self: Pin<&mut Self>, requestJson: &QString);
        #[qinvokable]
        #[cxx_name = "importEnvironmentFile"]
        fn import_environment_file(self: Pin<&mut Self>, source: &QString);

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
        self.as_mut().set_creating(false);
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
        self.as_mut().set_creating(false);
        let mut state = self.as_mut().rust_mut().state.clone();
        state.refresh_generation = state.refresh_generation.wrapping_add(1);
        state.selection_generation = state.selection_generation.wrapping_add(1);
        state.operation_generation = state.operation_generation.wrapping_add(1);
        state.operations.clear();
        state.group_operations.clear();
        state.refresh_in_progress = false;
        self.as_mut().apply_state(state);
    }

    pub fn clear_create_error(mut self: Pin<&mut Self>) {
        self.as_mut().set_create_error_message(QString::default());
    }

    pub fn create_container(mut self: Pin<&mut Self>, request_json: &QString) {
        if self.creating {
            return;
        }
        let request_json = request_json.to_string();
        let request = match validated_create_request(&request_json) {
            Ok(request) => request,
            Err(message) => {
                self.as_mut()
                    .set_create_error_message(QString::from(message));
                return;
            }
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .set_create_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let image_reference = request.image.clone();
        let (generation, cancel) = self.as_mut().begin_create_operation();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let inspected = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.images.inspect_image(&image_reference) => result,
            };
            match inspected {
                Err(DockerError::ImageNotFound(_)) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    qt_thread
                        .queue(move |mut model| {
                            if !model.as_mut().finish_create_operation(generation) {
                                return;
                            }
                            model.as_mut().image_pull_required(
                                QString::from(request_json),
                                QString::from(image_reference),
                            );
                        })
                        .ok();
                }
                Err(error) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    qt_thread
                        .queue(move |mut model| {
                            if !model.as_mut().finish_create_operation(generation) {
                                return;
                            }
                            model.as_mut().set_create_error_message(QString::from(
                                create_error_message(&error),
                            ));
                        })
                        .ok();
                }
                Ok(_) => {
                    let result = tokio::select! {
                        _ = cancel.cancelled() => return,
                        result = services.containers.create_container(&request) => result,
                    };
                    if cancel.is_cancelled() {
                        return;
                    }
                    queue_create_result(qt_thread, generation, result);
                }
            }
        });
    }

    pub fn confirm_pull_and_create(mut self: Pin<&mut Self>, request_json: &QString) {
        if self.creating {
            return;
        }
        // Treat confirmation JSON as opaque: parse and validate the supplied
        // copy again instead of trusting anything from the previous request.
        let request = match validated_create_request(&request_json.to_string()) {
            Ok(request) => request,
            Err(message) => {
                self.as_mut()
                    .set_create_error_message(QString::from(message));
                return;
            }
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .set_create_error_message(QString::from("Docker Engine is unavailable."));
            return;
        };
        let options = PullImageOptions {
            reference: request.image.clone(),
            platform: request.platform.clone(),
            registry_auth: None,
        };
        let mut stream = services.images.pull_image(options);
        let cancel = stream.cancellation_token();
        let generation = self.as_mut().begin_create_operation_with(cancel.clone());
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let pull_result = loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    item = stream.next() => match item {
                        Some(Ok(_)) => {}
                        Some(Err(error)) => break Err(error),
                        None => break Ok(()),
                    }
                }
            };
            if let Err(error) = pull_result {
                if cancel.is_cancelled() {
                    return;
                }
                qt_thread
                    .queue(move |mut model| {
                        if !model.as_mut().finish_create_operation(generation) {
                            return;
                        }
                        model
                            .as_mut()
                            .set_create_error_message(QString::from(create_error_message(&error)));
                    })
                    .ok();
                return;
            }
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = services.containers.create_container(&request) => result,
            };
            if cancel.is_cancelled() {
                return;
            }
            queue_create_result(qt_thread, generation, result);
        });
    }

    pub fn import_environment_file(mut self: Pin<&mut Self>, source: &QString) {
        self.as_mut().invalidate_environment_import();
        if !self.local_endpoint {
            self.as_mut().environment_file_import_failed(QString::from(
                "Environment files can only be imported for a local Docker endpoint.",
            ));
            return;
        }
        let path = match local_environment_path(&source.to_string()) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut()
                    .environment_file_import_failed(QString::from(message));
                return;
            }
        };
        let source_name = environment_source_name(&path);
        let (generation, cancel) = self.as_mut().begin_environment_import();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = cancel.cancelled() => return,
                result = read_environment_file(&path) => result,
            };
            if cancel.is_cancelled() {
                return;
            }
            qt_thread
                .queue(move |mut model| {
                    if model.environment_import_generation != generation {
                        return;
                    }
                    model.as_mut().rust_mut().environment_import_cancel = None;
                    match result {
                        Ok(entries) => {
                            let message = format!(
                                "Imported {} environment {} from {source_name}.",
                                entries.len(),
                                if entries.len() == 1 {
                                    "variable"
                                } else {
                                    "variables"
                                }
                            );
                            model.as_mut().environment_file_imported(
                                environment_entry_variants(&entries),
                                QString::from(message),
                            );
                        }
                        Err(message) => model
                            .as_mut()
                            .environment_file_import_failed(QString::from(message)),
                    }
                })
                .ok();
        });
    }

    fn begin_create_operation(mut self: Pin<&mut Self>) -> (u64, CancellationToken) {
        let cancel = CancellationToken::new();
        let generation = self.as_mut().begin_create_operation_with(cancel.clone());
        (generation, cancel)
    }

    fn begin_create_operation_with(mut self: Pin<&mut Self>, cancel: CancellationToken) -> u64 {
        if let Some(previous) = self.as_mut().rust_mut().create_cancel.take() {
            previous.cancel();
        }
        let generation = self.create_generation.wrapping_add(1);
        self.as_mut().rust_mut().create_generation = generation;
        self.as_mut().rust_mut().create_cancel = Some(cancel);
        self.as_mut().set_create_error_message(QString::default());
        self.as_mut().set_creating(true);
        generation
    }

    fn finish_create_operation(mut self: Pin<&mut Self>, generation: u64) -> bool {
        if self.create_generation != generation {
            return false;
        }
        self.as_mut().rust_mut().create_cancel = None;
        self.as_mut().set_creating(false);
        true
    }

    fn invalidate_environment_import(mut self: Pin<&mut Self>) {
        if let Some(previous) = self.as_mut().rust_mut().environment_import_cancel.take() {
            previous.cancel();
        }
        self.as_mut().rust_mut().environment_import_generation =
            self.environment_import_generation.wrapping_add(1);
    }

    fn begin_environment_import(mut self: Pin<&mut Self>) -> (u64, CancellationToken) {
        let generation = self.environment_import_generation.wrapping_add(1);
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().environment_import_generation = generation;
        self.as_mut().rust_mut().environment_import_cancel = Some(cancel.clone());
        (generation, cancel)
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
                            model
                                .as_mut()
                                .remove_container_prepared(remove_preparation_payload(&view));
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
            let explicit_targets = targets.clone();
            let executed = tokio::select! {
                _ = cancel.cancelled() => return,
                result = execute_compose_action(
                    &services,
                    &group_id,
                    &explicit_targets,
                    &action,
                ) => result,
            };
            let results: Vec<GroupTargetResult> = match executed {
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

    fn compose_action(&self) -> ComposeGroupAction {
        match self {
            Self::Start => ComposeGroupAction::Start,
            Self::Stop => ComposeGroupAction::Stop(StopContainerOptions {
                timeout_seconds: Some(10),
            }),
            Self::Restart => ComposeGroupAction::Restart(RestartContainerOptions {
                timeout_seconds: Some(10),
            }),
            Self::Pause => ComposeGroupAction::Pause,
            Self::Unpause => ComposeGroupAction::Unpause,
            Self::Remove {
                force,
                remove_volumes,
            } => ComposeGroupAction::Remove(RemoveContainerOptions {
                force: *force,
                remove_volumes: *remove_volumes,
                remove_links: false,
            }),
        }
    }
}

async fn execute_compose_action(
    services: &tuxstack_docker_core::DockerServices,
    group_id: &tuxstack_docker_core::ContainerGroupId,
    targets: &[String],
    action: &GroupBridgeAction,
) -> Result<tuxstack_docker_core::ContainerGroupOperationResult, DockerError> {
    services
        .compose
        .execute_group_targets(group_id, targets, action.compose_action())
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

fn create_error_message(error: &DockerError) -> String {
    match error {
        DockerError::PermissionDenied => "Permission denied while accessing Docker.".into(),
        DockerError::EngineUnavailable | DockerError::SocketNotFound(_) => {
            "Docker Engine is unavailable.".into()
        }
        DockerError::ConnectionTimeout | DockerError::OperationTimeout => {
            "The Docker create operation timed out.".into()
        }
        DockerError::RegistryAuthenticationFailed => {
            "Registry authentication failed. Pull the image using authenticated image tools, then try again.".into()
        }
        DockerError::RegistryUnavailable(_) => "The image registry is unavailable.".into(),
        DockerError::InvalidImageReference(_) => "The image reference is invalid.".into(),
        DockerError::ImageNotFound(_) => "The requested image was not found.".into(),
        DockerError::PullFailed(_) => "Docker could not pull the requested image.".into(),
        DockerError::OperationCancelled => "The create operation was cancelled.".into(),
        _ => operation_error_message(error),
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

fn remove_preparation_payload(view: &ContainerDetailView) -> QVariant {
    let mut map = QVariantMap::default();
    for (key, value) in [
        ("id", view.id.as_str()),
        ("name", view.name.as_str()),
        ("image", view.image.as_str()),
        ("state", view.state_name.as_str()),
        ("composeProject", view.compose_project.as_str()),
    ] {
        insert(&mut map, key, value);
    }
    map.insert(
        QString::from("mounts"),
        QVariant::from(&mount_rows(&view.mounts)),
    );
    QVariant::from(&map)
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

fn validated_create_request(
    json: &str,
) -> Result<tuxstack_docker_core::CreateContainerRequest, String> {
    let request =
        parse_create_request(json).map_err(|_| "Invalid create request JSON.".to_string())?;
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn queue_create_result(
    qt_thread: cxx_qt::CxxQtThread<qobject::ContainersListModel>,
    generation: u64,
    result: Result<tuxstack_docker_core::CreateContainerResult, DockerError>,
) {
    qt_thread
        .queue(move |mut model| {
            if !model.as_mut().finish_create_operation(generation) {
                return;
            }
            match result {
                Ok(result) => {
                    let mut messages = result.warnings;
                    messages.extend(
                        result.network_failures.iter().map(|failure| {
                            format!("Network {}: {}", failure.network, failure.error)
                        }),
                    );
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
                Err(error) => model
                    .as_mut()
                    .set_create_error_message(QString::from(create_error_message(&error))),
            }
        })
        .ok();
}

const MAX_ENVIRONMENT_FILE_BYTES: u64 = 1024 * 1024;

#[derive(PartialEq, Eq)]
struct ImportedEnvironmentEntry {
    key: String,
    value: String,
}

impl std::fmt::Debug for ImportedEnvironmentEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImportedEnvironmentEntry")
            .field("key", &self.key)
            .field("value", &"<redacted>")
            .finish()
    }
}

async fn read_environment_file(path: &Path) -> Result<Vec<ImportedEnvironmentEntry>, String> {
    let source_name = environment_source_name(path);
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| format!("Could not read {source_name}."))?;
    let mut bytes = Vec::new();
    file.take(MAX_ENVIRONMENT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| format!("Could not read {source_name}."))?;
    validate_environment_file_size(bytes.len() as u64)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{source_name} is not a valid UTF-8 environment file."))?;
    parse_environment_file(&text).map_err(|message| format!("{source_name}: {message}"))
}

fn validate_environment_file_size(size: u64) -> Result<(), String> {
    if size > MAX_ENVIRONMENT_FILE_BYTES {
        Err("The environment file exceeds the 1 MiB limit.".into())
    } else {
        Ok(())
    }
}

fn parse_environment_file(text: &str) -> Result<Vec<ImportedEnvironmentEntry>, String> {
    let mut entries = Vec::new();
    let mut keys = HashSet::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let mut line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export") {
            if rest.starts_with(char::is_whitespace) {
                line = rest.trim_start();
            }
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(format!("Line {line_number}: expected KEY=VALUE."));
        };
        let key = raw_key.trim();
        if !valid_environment_key(key) {
            return Err(format!(
                "Line {line_number}: invalid environment variable key."
            ));
        }
        if !keys.insert(key.to_string()) {
            return Err(format!(
                "Line {line_number}: duplicate environment variable key {key}."
            ));
        }
        let value = parse_environment_value(raw_value, line_number)?;
        entries.push(ImportedEnvironmentEntry {
            key: key.to_string(),
            value,
        });
    }
    Ok(entries)
}

fn valid_environment_key(key: &str) -> bool {
    let mut characters = key.chars();
    matches!(characters.next(), Some('A'..='Z' | 'a'..='z' | '_'))
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn parse_environment_value(raw: &str, line_number: usize) -> Result<String, String> {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix('\'') {
        return parse_single_quoted_environment_value(rest, line_number);
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        return parse_double_quoted_environment_value(rest, line_number);
    }
    Ok(parse_unquoted_environment_value(raw))
}

fn parse_single_quoted_environment_value(
    value: &str,
    line_number: usize,
) -> Result<String, String> {
    let Some(end) = value.find('\'') else {
        return Err(format!(
            "Line {line_number}: unterminated single-quoted value."
        ));
    };
    validate_quoted_environment_suffix(&value[end + 1..], line_number)?;
    Ok(value[..end].to_string())
}

fn parse_double_quoted_environment_value(
    value: &str,
    line_number: usize,
) -> Result<String, String> {
    let mut result = String::new();
    let mut characters = value.char_indices();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => {
                validate_quoted_environment_suffix(&value[index + 1..], line_number)?;
                return Ok(result);
            }
            '\\' => {
                let Some((_, escaped)) = characters.next() else {
                    return Err(format!(
                        "Line {line_number}: unterminated double-quoted value."
                    ));
                };
                result.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    _ => {
                        return Err(format!(
                            "Line {line_number}: unsupported escape in double-quoted value."
                        ));
                    }
                });
            }
            _ => result.push(character),
        }
    }
    Err(format!(
        "Line {line_number}: unterminated double-quoted value."
    ))
}

fn validate_quoted_environment_suffix(suffix: &str, line_number: usize) -> Result<(), String> {
    if suffix.is_empty() {
        return Ok(());
    }
    if !suffix.starts_with(char::is_whitespace) {
        return Err(format!(
            "Line {line_number}: quoted value must contain the complete value."
        ));
    }
    let suffix = suffix.trim_start();
    if suffix.is_empty() || suffix.starts_with('#') {
        Ok(())
    } else {
        Err(format!(
            "Line {line_number}: unexpected text after quoted value."
        ))
    }
}

fn parse_unquoted_environment_value(value: &str) -> String {
    let mut previous_was_whitespace = false;
    let mut comment_start = None;
    for (index, character) in value.char_indices() {
        if character == '#' && previous_was_whitespace {
            comment_start = Some(index);
            break;
        }
        previous_was_whitespace = character.is_whitespace();
    }
    value[..comment_start.unwrap_or(value.len())]
        .trim()
        .to_string()
}

fn local_environment_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("An environment file is required.".into());
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        let encoded = if rest.starts_with('/') {
            rest
        } else if let Some((host, path)) = rest.split_once('/') {
            if host.eq_ignore_ascii_case("localhost") {
                return decoded_environment_path(&format!("/{path}"));
            }
            return Err("The environment file URL must refer to the local host.".into());
        } else {
            return Err("The environment file URL is malformed.".into());
        };
        reject_file_url_query_or_fragment(encoded)?;
        percent_decode_environment_path(encoded)?
    } else if let Some(encoded) = value.strip_prefix("file:") {
        if !encoded.starts_with('/') {
            return Err("The environment file URL is malformed.".into());
        }
        reject_file_url_query_or_fragment(encoded)?;
        percent_decode_environment_path(encoded)?
    } else if value.contains("://") {
        return Err("The environment file must be a local path.".into());
    } else {
        value.to_string()
    };
    validate_decoded_environment_path(path)
}

fn decoded_environment_path(value: &str) -> Result<PathBuf, String> {
    reject_file_url_query_or_fragment(value)?;
    validate_decoded_environment_path(percent_decode_environment_path(value)?)
}

fn reject_file_url_query_or_fragment(value: &str) -> Result<(), String> {
    if value.contains(['?', '#']) {
        Err("The environment file URL is malformed.".into())
    } else {
        Ok(())
    }
}

fn validate_decoded_environment_path(path: String) -> Result<PathBuf, String> {
    if path.is_empty() || path.contains('\0') {
        Err("The environment file path is invalid.".into())
    } else {
        Ok(PathBuf::from(path))
    }
}

fn percent_decode_environment_path(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("The environment file URL contains a malformed escape.".into());
            }
            let (Some(high), Some(low)) = (
                hexadecimal_digit(bytes[index + 1]),
                hexadecimal_digit(bytes[index + 2]),
            ) else {
                return Err("The environment file URL contains a malformed escape.".into());
            };
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output)
        .map_err(|_| "The environment file URL is not valid UTF-8.".to_string())
}

fn hexadecimal_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn environment_source_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("environment file")
        .to_string()
}

fn environment_entry_variants(entries: &[ImportedEnvironmentEntry]) -> QVariantList {
    entries
        .iter()
        .map(|entry| variant_map(&[("key", &entry.key), ("value", &entry.value)]))
        .collect()
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
    if let Some(cancel) = rust.create_cancel.take() {
        cancel.cancel();
    }
    rust.create_generation = rust.create_generation.wrapping_add(1);
    if let Some(cancel) = rust.environment_import_cancel.take() {
        cancel.cancel();
    }
    rust.environment_import_generation = rust.environment_import_generation.wrapping_add(1);
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
    fn group_actions_map_to_explicit_target_compose_actions() {
        assert_eq!(
            GroupBridgeAction::Start.compose_action(),
            ComposeGroupAction::Start
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
            .compose_action(),
            ComposeGroupAction::Remove(RemoveContainerOptions {
                force: true,
                remove_volumes: false,
                remove_links: false,
            })
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

    #[test]
    fn dotenv_parser_handles_comments_empty_export_and_unquoted_hashes() {
        let entries = parse_environment_file(
            "\n  # comment\nexport FIRST=one\nEMPTY=\nHASH=left#right\nCOMMENT=value # ignored\n",
        )
        .unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].key, "FIRST");
        assert_eq!(entries[0].value, "one");
        assert_eq!(entries[1].value, "");
        assert_eq!(entries[2].value, "left#right");
        assert_eq!(entries[3].value, "value");
        assert!(
            parse_environment_file("\n # only a comment\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dotenv_parser_supports_complete_quotes_and_basic_double_quote_escapes() {
        let entries = parse_environment_file(
            "SINGLE=' literal # value ' # comment\nDOUBLE=\"line\\nquote: \\\" slash: \\\\ tab: \\t\"\n",
        )
        .unwrap();
        assert_eq!(entries[0].value, " literal # value ");
        assert_eq!(entries[1].value, "line\nquote: \" slash: \\ tab: \t");
    }

    #[test]
    fn dotenv_parser_rejects_duplicates_and_malformed_lines_with_line_numbers() {
        let duplicate = parse_environment_file("A=one\n# skipped\nA=two\n").unwrap_err();
        assert!(duplicate.contains("Line 3"));
        assert!(duplicate.contains("duplicate"));
        for malformed in [
            "NO_EQUALS",
            "9KEY=value",
            "KEY='unterminated",
            "KEY=\"bad\\q\"",
            "KEY='value'trailing",
            "export =value",
        ] {
            let error = parse_environment_file(malformed).unwrap_err();
            assert!(error.contains("Line 1"), "{error}");
        }
    }

    #[test]
    fn environment_file_size_limit_is_exact() {
        assert!(validate_environment_file_size(MAX_ENVIRONMENT_FILE_BYTES).is_ok());
        assert!(validate_environment_file_size(MAX_ENVIRONMENT_FILE_BYTES + 1).is_err());
    }

    #[test]
    fn environment_paths_decode_local_file_urls_and_reject_unsafe_inputs() {
        assert_eq!(
            local_environment_path("file:///tmp/My%20Environment.env").unwrap(),
            PathBuf::from("/tmp/My Environment.env")
        );
        assert_eq!(
            local_environment_path("file://localhost/tmp/config.env").unwrap(),
            PathBuf::from("/tmp/config.env")
        );
        assert_eq!(
            local_environment_path("/tmp/100%.env").unwrap(),
            PathBuf::from("/tmp/100%.env")
        );
        for invalid in [
            "file://remote/tmp/config.env",
            "https://example.invalid/config.env",
            "file:///tmp/bad%2",
            "file:///tmp/bad%GG",
            "file:///tmp/non-utf8-%FF",
            "file:///tmp/nul%00.env",
            "file:///tmp/config.env?query",
            "file://localhost",
        ] {
            assert!(local_environment_path(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn imported_environment_debug_redacts_values() {
        let entry = ImportedEnvironmentEntry {
            key: "TOKEN".into(),
            value: "top-secret-value".into(),
        };
        let debug = format!("{entry:?}");
        assert!(debug.contains("TOKEN"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("top-secret-value"));
    }

    #[tokio::test]
    async fn environment_reader_rejects_non_utf8_without_disclosing_bytes() {
        let path = std::env::temp_dir().join(format!(
            "tuxstack-containers-env-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::write(&path, b"TOKEN=secret\nBAD=\xff\n")
            .await
            .unwrap();
        let error = read_environment_file(&path).await.unwrap_err();
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(error.contains("not a valid UTF-8"));
        assert!(!error.contains("secret"));
    }

    #[test]
    fn create_errors_do_not_disclose_pull_or_invalid_json_values() {
        let error = create_error_message(&DockerError::PullFailed(
            "authorization header token=top-secret".into(),
        ));
        assert_eq!(error, "Docker could not pull the requested image.");
        assert!(!error.contains("top-secret"));

        let invalid =
            validated_create_request(r#"{"image":"nginx","tty":"top-secret"}"#).unwrap_err();
        assert_eq!(invalid, "Invalid create request JSON.");
        assert!(!invalid.contains("top-secret"));
    }
}
