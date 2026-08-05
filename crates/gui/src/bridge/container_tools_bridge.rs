//! Container Files CXX-Qt bridge.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    ContainerFilePreview, ContainerFilesystemEntry, ContainerFilesystemError,
    ContainerFilesystemOrigin, ContainerFilesystemSnapshot, ContainerMountOverlayKind,
};

use crate::app_state::get_services;
use crate::controllers::container_files::{
    ContainerFileSortColumn, ContainerFilesControllerState, ContainerFilesState, MountAction,
    entry_type_name,
};

const ROLE_NAME: i32 = 257;
const ROLE_PATH: i32 = 258;
const ROLE_ENTRY_TYPE: i32 = 259;
const ROLE_ICON_NAME: i32 = 260;
const ROLE_SIZE_BYTES: i32 = 261;
const ROLE_SIZE_TEXT: i32 = 262;
const ROLE_MODIFIED_TEXT: i32 = 263;
const ROLE_MODE_TEXT: i32 = 264;
const ROLE_OWNER_TEXT: i32 = 265;
const ROLE_LINK_TARGET: i32 = 266;
const ROLE_SELECTED: i32 = 267;
const ROLE_ORIGIN: i32 = 268;
const ROLE_MOUNT_KIND: i32 = 269;
const ROLE_MOUNT_SOURCE: i32 = 270;
const ROLE_MOUNT_DESTINATION: i32 = 271;
const ROLE_MOUNT_READ_ONLY: i32 = 272;
const ROLE_MOUNT_ACTION: i32 = 273;

#[derive(Debug, Clone, Default)]
pub struct ContainerFileRow {
    name: String,
    path: String,
    entry_type: String,
    icon_name: String,
    size_bytes: i64,
    size_text: String,
    modified_text: String,
    mode_text: String,
    owner_text: String,
    link_target: String,
    origin: String,
    mount_kind: String,
    mount_source: String,
    mount_destination: String,
    mount_read_only: bool,
    mount_action: String,
}

#[derive(Default)]
pub struct ContainerFileListModelRust {
    pub(crate) state: ContainerFilesControllerState,
    pub(crate) snapshot: Option<Arc<ContainerFilesystemSnapshot>>,
    pub(crate) rows: Vec<ContainerFileRow>,

    pub(crate) files_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) container_id: QString,
    pub(crate) current_path: QString,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_up: bool,
    pub(crate) show_hidden: bool,
    pub(crate) search_query: QString,
    pub(crate) sort_column: QString,
    pub(crate) sort_descending: bool,
    pub(crate) selected_entry_path: QString,
    pub(crate) loading: bool,
    pub(crate) refreshing_snapshot: bool,
    pub(crate) count: i32,
    pub(crate) total_visible: i32,
    pub(crate) has_more: bool,
    pub(crate) active: bool,
    pub(crate) snapshot_generated_at: QString,
    pub(crate) snapshot_status: QString,
    pub(crate) snapshot_stale: bool,

    pub(crate) preview_loading: bool,
    pub(crate) preview_name: QString,
    pub(crate) preview_path: QString,
    pub(crate) preview_text: QString,
    pub(crate) preview_size_text: QString,
    pub(crate) preview_truncated: bool,
    pub(crate) preview_binary: bool,
    pub(crate) preview_error: QString,

    pub(crate) save_in_progress: bool,
    pub(crate) save_error: QString,
    pub(crate) properties: QList<QVariant>,

    pub(crate) snapshot_cancel: Option<CancellationToken>,
    pub(crate) list_cancel: Option<CancellationToken>,
    pub(crate) preview_cancel: Option<CancellationToken>,
    pub(crate) save_cancel: Option<CancellationToken>,
}

impl ContainerFileListModelRust {
    fn cancel_all(&mut self) {
        cancel(&mut self.snapshot_cancel);
        cancel(&mut self.list_cancel);
        cancel(&mut self.preview_cancel);
        cancel(&mut self.save_cancel);
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
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/core/qlist/qlist_QVariant.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    impl cxx_qt::Threading for ContainerFileListModel {}

    unsafe extern "RustQt" {
        /// Point-in-time merged-rootfs browser. Mounted destinations are
        /// inspect-derived navigation overlays, never exported shadow content.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, files_state, cxx_name = "filesState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, container_id, cxx_name = "containerId")]
        #[qproperty(QString, current_path, cxx_name = "currentPath")]
        #[qproperty(bool, can_go_back, cxx_name = "canGoBack")]
        #[qproperty(bool, can_go_up, cxx_name = "canGoUp")]
        #[qproperty(bool, show_hidden, cxx_name = "showHidden", READ, NOTIFY)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn")]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending")]
        #[qproperty(QString, selected_entry_path, cxx_name = "selectedEntryPath")]
        #[qproperty(bool, loading)]
        #[qproperty(bool, refreshing_snapshot, cxx_name = "refreshingSnapshot")]
        #[qproperty(i32, count)]
        #[qproperty(i32, total_visible, cxx_name = "totalVisible")]
        #[qproperty(bool, has_more, cxx_name = "hasMore")]
        #[qproperty(bool, active, READ, NOTIFY)]
        #[qproperty(QString, snapshot_generated_at, cxx_name = "snapshotGeneratedAt")]
        #[qproperty(QString, snapshot_status, cxx_name = "snapshotStatus")]
        #[qproperty(bool, snapshot_stale, cxx_name = "snapshotStale")]
        #[qproperty(bool, preview_loading, cxx_name = "previewLoading")]
        #[qproperty(QString, preview_name, cxx_name = "previewName")]
        #[qproperty(QString, preview_path, cxx_name = "previewPath")]
        #[qproperty(QString, preview_text, cxx_name = "previewText")]
        #[qproperty(QString, preview_size_text, cxx_name = "previewSizeText")]
        #[qproperty(bool, preview_truncated, cxx_name = "previewTruncated")]
        #[qproperty(bool, preview_binary, cxx_name = "previewBinary")]
        #[qproperty(QString, preview_error, cxx_name = "previewError")]
        #[qproperty(bool, save_in_progress, cxx_name = "saveInProgress")]
        #[qproperty(QString, save_error, cxx_name = "saveError")]
        #[qproperty(QList_QVariant, properties)]
        type ContainerFileListModel = super::ContainerFileListModelRust;

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

        #[qsignal]
        #[cxx_name = "volumeMountRequested"]
        fn volume_mount_requested(self: Pin<&mut Self>, volume_name: QString);
        #[qsignal]
        #[cxx_name = "bindMountRequested"]
        fn bind_mount_requested(self: Pin<&mut Self>, source_path: QString);
        #[qsignal]
        #[cxx_name = "tmpfsMountRequested"]
        fn tmpfs_mount_requested(self: Pin<&mut Self>, destination: QString, read_only: bool);
        #[qsignal]
        #[cxx_name = "previewReady"]
        fn preview_ready(self: Pin<&mut Self>);
        #[qsignal]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut Self>, message: QString);
        #[qsignal]
        #[cxx_name = "saveCompleted"]
        fn save_completed(self: Pin<&mut Self>, destination: QString);
        #[qsignal]
        #[cxx_name = "saveFailed"]
        fn save_failed(self: Pin<&mut Self>, message: QString);
        #[qsignal]
        #[cxx_name = "propertiesReady"]
        fn properties_ready(self: Pin<&mut Self>);

        #[qinvokable]
        #[rust_name = "update_active"]
        #[cxx_name = "setActive"]
        fn set_active(self: Pin<&mut Self>, active: bool);
        #[qinvokable]
        #[cxx_name = "selectContainer"]
        fn select_container(self: Pin<&mut Self>, container_id: &QString);
        #[qinvokable]
        #[cxx_name = "clearSelection"]
        fn clear_selection(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshSnapshot"]
        fn refresh_snapshot(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "invalidateSnapshot"]
        fn invalidate_snapshot(self: Pin<&mut Self>, container_id: &QString);
        #[qinvokable]
        #[cxx_name = "openEntry"]
        fn open_entry(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        #[cxx_name = "selectEntry"]
        fn select_entry(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        #[cxx_name = "goBack"]
        fn go_back(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "goUp"]
        fn go_up(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "navigateTo"]
        fn navigate_to(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        #[rust_name = "update_show_hidden"]
        #[cxx_name = "setShowHidden"]
        fn set_show_hidden(self: Pin<&mut Self>, show: bool);
        #[qinvokable]
        #[rust_name = "update_search_query"]
        #[cxx_name = "setSearchQuery"]
        fn set_search_query(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        #[cxx_name = "toggleSort"]
        fn toggle_sort(self: Pin<&mut Self>, column: &QString);
        #[qinvokable]
        #[cxx_name = "loadMore"]
        fn load_more(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "previewEntry"]
        fn preview_entry(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        #[cxx_name = "cancelPreview"]
        fn cancel_preview(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "saveEntry"]
        fn save_entry(self: Pin<&mut Self>, path: &QString, destination: &QString);
        #[qinvokable]
        #[cxx_name = "cancelSave"]
        fn cancel_save(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "loadProperties"]
        fn load_properties(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        #[cxx_name = "updateSnapshotClock"]
        fn update_snapshot_clock(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32);
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }
}

impl qobject::ContainerFileListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            ROLE_NAME => qv(&row.name),
            ROLE_PATH => qv(&row.path),
            ROLE_ENTRY_TYPE => qv(&row.entry_type),
            ROLE_ICON_NAME => qv(&row.icon_name),
            ROLE_SIZE_BYTES => QVariant::from(&row.size_bytes),
            ROLE_SIZE_TEXT => qv(&row.size_text),
            ROLE_MODIFIED_TEXT => qv(&row.modified_text),
            ROLE_MODE_TEXT => qv(&row.mode_text),
            ROLE_OWNER_TEXT => qv(&row.owner_text),
            ROLE_LINK_TARGET => qv(&row.link_target),
            ROLE_SELECTED => {
                QVariant::from(&(self.state.selected_path.as_deref() == Some(&row.path)))
            }
            ROLE_ORIGIN => qv(&row.origin),
            ROLE_MOUNT_KIND => qv(&row.mount_kind),
            ROLE_MOUNT_SOURCE => qv(&row.mount_source),
            ROLE_MOUNT_DESTINATION => qv(&row.mount_destination),
            ROLE_MOUNT_READ_ONLY => QVariant::from(&row.mount_read_only),
            ROLE_MOUNT_ACTION => qv(&row.mount_action),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (ROLE_NAME, "name"),
            (ROLE_PATH, "path"),
            (ROLE_ENTRY_TYPE, "entryType"),
            (ROLE_ICON_NAME, "iconName"),
            (ROLE_SIZE_BYTES, "sizeBytes"),
            (ROLE_SIZE_TEXT, "sizeText"),
            (ROLE_MODIFIED_TEXT, "modifiedText"),
            (ROLE_MODE_TEXT, "modeText"),
            (ROLE_OWNER_TEXT, "ownerText"),
            (ROLE_LINK_TARGET, "linkTarget"),
            (ROLE_SELECTED, "selected"),
            (ROLE_ORIGIN, "origin"),
            (ROLE_MOUNT_KIND, "mountKind"),
            (ROLE_MOUNT_SOURCE, "mountSource"),
            (ROLE_MOUNT_DESTINATION, "mountDestination"),
            (ROLE_MOUNT_READ_ONLY, "mountReadOnly"),
            (ROLE_MOUNT_ACTION, "mountAction"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    pub(crate) fn update_active(mut self: Pin<&mut Self>, active: bool) {
        self.as_mut().rust_mut().state.set_active(active);
        if active && !self.state.container_id.is_empty() {
            self.as_mut().ensure_snapshot(false);
        } else {
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn select_container(mut self: Pin<&mut Self>, container_id: &QString) {
        let changed = self
            .as_mut()
            .rust_mut()
            .state
            .select_container(&container_id.to_string());
        if changed {
            self.as_mut().rust_mut().cancel_all();
            self.as_mut().rust_mut().snapshot = None;
        }
        if self.state.active && !self.state.container_id.is_empty() {
            self.as_mut().ensure_snapshot(false);
        } else {
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn clear_selection(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().cancel_all();
        self.as_mut().rust_mut().snapshot = None;
        self.as_mut().rust_mut().state.clear_selection();
        self.as_mut().reset_preview();
        self.as_mut().publish_state();
    }

    pub(crate) fn refresh_snapshot(self: Pin<&mut Self>) {
        self.ensure_snapshot(true);
    }

    pub(crate) fn invalidate_snapshot(mut self: Pin<&mut Self>, container_id: &QString) {
        if self.state.container_id == container_id.to_string() {
            self.as_mut().rust_mut().cancel_all();
            self.as_mut().rust_mut().state.invalidate();
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn open_entry(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        let Some(entry) = self
            .state
            .entries
            .iter()
            .find(|entry| entry.logical_path == path)
            .cloned()
        else {
            return;
        };
        if let Some(action) = self.state.mount_action(&entry) {
            match action {
                MountAction::Volume { name } if !name.is_empty() => {
                    self.as_mut().volume_mount_requested(qs(&name));
                }
                MountAction::Bind { source } if !source.is_empty() => {
                    self.as_mut().bind_mount_requested(qs(&source));
                }
                MountAction::Tmpfs => {
                    if let Some((destination, read_only)) = self
                        .state
                        .mount_for_entry(&entry)
                        .map(|mount| (mount.destination.clone(), mount.read_only))
                    {
                        self.as_mut()
                            .tmpfs_mount_requested(qs(&destination), read_only);
                    }
                }
                _ => {}
            }
            return;
        }
        if entry.entry_type == tuxstack_docker_core::ContainerFilesystemEntryType::Directory {
            if self.as_mut().rust_mut().state.navigate_to(&path, true) {
                self.as_mut().load_directory();
            }
        } else {
            self.as_mut().preview_entry(&qs(&path));
        }
    }

    pub(crate) fn select_entry(mut self: Pin<&mut Self>, path: &QString) {
        self.as_mut()
            .rust_mut()
            .state
            .select_path(Some(&path.to_string()));
        self.as_mut().publish_state();
    }

    pub(crate) fn go_back(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().state.go_back() {
            self.as_mut().load_directory();
        }
    }

    pub(crate) fn go_up(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().state.go_up() {
            self.as_mut().load_directory();
        }
    }

    pub(crate) fn navigate_to(mut self: Pin<&mut Self>, path: &QString) {
        if self
            .as_mut()
            .rust_mut()
            .state
            .navigate_to(&path.to_string(), true)
        {
            self.as_mut().load_directory();
        }
    }

    pub(crate) fn update_show_hidden(mut self: Pin<&mut Self>, show: bool) {
        if self.as_mut().rust_mut().state.set_show_hidden(show) {
            self.as_mut().load_directory();
        }
    }

    pub(crate) fn update_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut()
            .rust_mut()
            .state
            .set_search(&query.to_string());
        self.as_mut().publish_state();
    }

    pub(crate) fn toggle_sort(mut self: Pin<&mut Self>, column: &QString) {
        self.as_mut()
            .rust_mut()
            .state
            .toggle_sort(ContainerFileSortColumn::parse(&column.to_string()));
        self.as_mut().load_directory();
    }

    pub(crate) fn load_more(mut self: Pin<&mut Self>) {
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let Some((generation, query)) = self.as_mut().rust_mut().state.begin_more() else {
            return;
        };
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().list_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = tokio::task::spawn_blocking(move || snapshot.list_directory(&query)) => {
                    result.unwrap_or_else(|error| Err(ContainerFilesystemError::MalformedArchive(error.to_string())))
                }
            };
            qt.queue(move |mut model| {
                let mut state = model.as_mut().rust_mut().state.clone();
                match result {
                    Ok(page) => {
                        state.apply_more(generation, page);
                    }
                    Err(error) => {
                        let (kind, message) = map_files_error(&error);
                        state.apply_list_error(generation, kind, &message);
                    }
                }
                model.as_mut().rust_mut().state = state;
                model.as_mut().publish_state();
            })
            .ok();
        });
    }

    pub(crate) fn preview_entry(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        if path.is_empty() || self.state.container_id.is_empty() {
            return;
        }
        let Some(services) = get_services() else {
            self.as_mut()
                .preview_failed(qs("Docker Engine is unavailable."));
            return;
        };
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        let generation = self.as_mut().rust_mut().state.begin_preview();
        self.as_mut().rust_mut().preview_loading = true;
        self.as_mut().rust_mut().preview_path = qs(&path);
        self.as_mut().rust_mut().preview_name = qs(path.rsplit('/').next().unwrap_or(&path));
        self.as_mut().rust_mut().preview_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().preview_cancel = Some(token.clone());
        let container_id = self.state.container_id.clone();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services
                .container_files
                .preview_file(&container_id, &path, None, token)
                .await;
            qt.queue(move |mut model| {
                if generation != model.state.preview_generation {
                    return;
                }
                model.as_mut().rust_mut().preview_loading = false;
                match result {
                    Ok(preview) => model.as_mut().apply_preview(preview),
                    Err(error) => {
                        let (_, message) = map_files_error(&error);
                        model.as_mut().rust_mut().preview_error = qs(&message);
                        model.as_mut().preview_failed(qs(&message));
                    }
                }
            })
            .ok();
        });
    }

    pub(crate) fn cancel_preview(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        self.as_mut().rust_mut().state.begin_preview();
        self.as_mut().reset_preview();
    }

    pub(crate) fn save_entry(mut self: Pin<&mut Self>, path: &QString, destination: &QString) {
        let path = path.to_string();
        let destination = match local_destination(&destination.to_string()) {
            Ok(destination) => destination,
            Err(message) => {
                self.as_mut().save_failed(qs(&message));
                return;
            }
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .save_failed(qs("Docker Engine is unavailable."));
            return;
        };
        cancel(&mut self.as_mut().rust_mut().save_cancel);
        let generation = self.as_mut().rust_mut().state.begin_save();
        self.as_mut().rust_mut().save_in_progress = true;
        self.as_mut().rust_mut().save_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().save_cancel = Some(token.clone());
        let container_id = self.state.container_id.clone();
        let shown_destination = destination.display().to_string();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services
                .container_files
                .save_file(&container_id, &path, &destination, token)
                .await;
            qt.queue(move |mut model| {
                if generation != model.state.save_generation {
                    return;
                }
                model.as_mut().rust_mut().save_in_progress = false;
                match result {
                    Ok(_) => model.as_mut().save_completed(qs(&shown_destination)),
                    Err(error) => {
                        let (_, message) = map_files_error(&error);
                        model.as_mut().rust_mut().save_error = qs(&message);
                        model.as_mut().save_failed(qs(&message));
                    }
                }
            })
            .ok();
        });
    }

    pub(crate) fn cancel_save(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().save_cancel);
        self.as_mut().rust_mut().state.begin_save();
        self.as_mut().rust_mut().save_in_progress = false;
    }

    pub(crate) fn load_properties(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        let Some(entry) = self
            .state
            .entries
            .iter()
            .find(|entry| entry.logical_path == path)
        else {
            return;
        };
        let row = map_row(entry, &self.state);
        let mut properties = QList::<QVariant>::default();
        push_property(&mut properties, "Path", &row.path);
        push_property(&mut properties, "Type", &row.entry_type);
        push_property(&mut properties, "Size", &row.size_text);
        push_property(&mut properties, "Modified", &row.modified_text);
        push_property(&mut properties, "Mode", &row.mode_text);
        push_property(&mut properties, "Owner", &row.owner_text);
        if !row.link_target.is_empty() {
            push_property(&mut properties, "Link Target", &row.link_target);
        }
        if !row.mount_kind.is_empty() {
            push_property(&mut properties, "Mount Type", &row.mount_kind);
            push_property(&mut properties, "Mount Source", &row.mount_source);
            push_property(&mut properties, "Mount Destination", &row.mount_destination);
            push_property(
                &mut properties,
                "Access",
                if row.mount_read_only {
                    "Read only"
                } else {
                    "Read/write"
                },
            );
            push_property(
                &mut properties,
                "Snapshot Note",
                "Mounted content is not included in this root filesystem snapshot.",
            );
        }
        self.as_mut().set_properties(properties);
        self.as_mut().properties_ready();
    }

    pub(crate) fn update_snapshot_clock(mut self: Pin<&mut Self>) {
        let now = chrono::Utc::now();
        let status = self.state.snapshot_status_text(now);
        let stale =
            self.state.snapshot_generated_at.is_some() && !self.state.snapshot_is_fresh_at(now);
        self.as_mut().set_snapshot_status(qs(&status));
        self.as_mut().set_snapshot_stale(stale);
    }

    pub(crate) fn set_connection_state(mut self: Pin<&mut Self>, docker_status: i32) {
        if docker_status != 1 {
            self.as_mut().rust_mut().cancel_all();
            self.as_mut().rust_mut().state.invalidate();
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().cancel_all();
        self.as_mut().rust_mut().snapshot = None;
        self.as_mut().rust_mut().state.clear_selection();
        self.as_mut().reset_preview();
        self.as_mut().publish_state();
    }

    fn ensure_snapshot(mut self: Pin<&mut Self>, force: bool) {
        let Some(services) = get_services() else {
            let generation = self
                .as_mut()
                .rust_mut()
                .state
                .begin_snapshot(chrono::Utc::now(), force)
                .unwrap_or(self.state.snapshot_generation);
            self.as_mut().rust_mut().state.apply_snapshot_error(
                generation,
                "docker_unavailable",
                "Docker Engine is unavailable.",
            );
            self.as_mut().publish_state();
            return;
        };
        let Some(generation) = self
            .as_mut()
            .rust_mut()
            .state
            .begin_snapshot(chrono::Utc::now(), force)
        else {
            self.as_mut().publish_state();
            return;
        };
        cancel(&mut self.as_mut().rust_mut().snapshot_cancel);
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().snapshot_cancel = Some(token.clone());
        let container_id = self.state.container_id.clone();
        self.as_mut().publish_state();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services
                .container_files
                .snapshot(&container_id, token)
                .await;
            qt.queue(move |mut model| match result {
                Ok(snapshot) => {
                    if generation != model.state.snapshot_generation {
                        return;
                    }
                    let generated_at = snapshot.generated_at;
                    let overlays = snapshot.mount_overlays.clone();
                    model.as_mut().rust_mut().snapshot = Some(Arc::new(snapshot));
                    let applied = model.as_mut().rust_mut().state.apply_snapshot(
                        generation,
                        generated_at,
                        overlays,
                    );
                    if applied {
                        model.as_mut().load_directory();
                    }
                }
                Err(error) => {
                    let (kind, message) = map_files_error(&error);
                    model
                        .as_mut()
                        .rust_mut()
                        .state
                        .apply_snapshot_error(generation, kind, &message);
                    model.as_mut().publish_state();
                }
            })
            .ok();
        });
    }

    fn load_directory(mut self: Pin<&mut Self>) {
        let Some(snapshot) = self.snapshot.clone() else {
            self.as_mut().publish_state();
            return;
        };
        let Some((generation, query)) = self.as_mut().rust_mut().state.begin_list() else {
            self.as_mut().publish_state();
            return;
        };
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().list_cancel = Some(token.clone());
        self.as_mut().publish_state();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = tokio::task::spawn_blocking(move || snapshot.list_directory(&query)) => {
                    result.unwrap_or_else(|error| Err(ContainerFilesystemError::MalformedArchive(error.to_string())))
                }
            };
            qt.queue(move |mut model| {
                let mut state = model.as_mut().rust_mut().state.clone();
                match result {
                    Ok(page) => {
                        state.apply_page(generation, page);
                    }
                    Err(error) => {
                        let (kind, message) = map_files_error(&error);
                        state.apply_list_error(generation, kind, &message);
                    }
                }
                model.as_mut().rust_mut().state = state;
                model.as_mut().publish_state();
            })
            .ok();
        });
    }

    fn apply_preview(mut self: Pin<&mut Self>, preview: ContainerFilePreview) {
        self.as_mut().rust_mut().preview_size_text = qs(&format_bytes(preview.file_size));
        self.as_mut().rust_mut().preview_truncated = preview.truncated;
        match String::from_utf8(preview.bytes) {
            Ok(text) if !text.contains('\0') => {
                self.as_mut().rust_mut().preview_binary = false;
                self.as_mut().rust_mut().preview_text = qs(&text);
            }
            _ => {
                self.as_mut().rust_mut().preview_binary = true;
                self.as_mut().rust_mut().preview_text = QString::default();
            }
        }
        self.as_mut().preview_ready();
    }

    fn reset_preview(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        rust.preview_loading = false;
        rust.preview_name = QString::default();
        rust.preview_path = QString::default();
        rust.preview_text = QString::default();
        rust.preview_size_text = QString::default();
        rust.preview_truncated = false;
        rust.preview_binary = false;
        rust.preview_error = QString::default();
    }

    fn publish_state(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        let rows = state
            .visible_entries()
            .into_iter()
            .map(|entry| map_row(entry, &state))
            .collect::<Vec<_>>();
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();

        self.as_mut().set_files_state(qs(state.state.as_str()));
        self.as_mut().set_error_kind(qs(&state.error_kind));
        self.as_mut().set_error_message(qs(&state.error_message));
        self.as_mut().set_container_id(qs(&state.container_id));
        self.as_mut().set_current_path(qs(&state.current_path));
        self.as_mut().set_can_go_back(state.can_go_back());
        self.as_mut().set_can_go_up(state.can_go_up());
        if self.show_hidden != state.show_hidden {
            self.as_mut().rust_mut().show_hidden = state.show_hidden;
            self.as_mut().show_hidden_changed();
        }
        let search_query = qs(&state.search_query);
        if self.search_query != search_query {
            self.as_mut().rust_mut().search_query = search_query;
            self.as_mut().search_query_changed();
        }
        self.as_mut()
            .set_sort_column(qs(state.sort_column.as_str()));
        self.as_mut().set_sort_descending(state.sort_descending);
        self.as_mut()
            .set_selected_entry_path(qs(state.selected_path.as_deref().unwrap_or_default()));
        self.as_mut().set_loading(matches!(
            state.state,
            ContainerFilesState::LoadingSnapshot | ContainerFilesState::LoadingDirectory
        ));
        self.as_mut()
            .set_refreshing_snapshot(state.snapshot_in_flight);
        let count = saturating_i32(self.rows.len());
        self.as_mut().set_count(count);
        self.as_mut()
            .set_total_visible(saturating_i32(state.total_visible));
        self.as_mut().set_has_more(state.next_cursor.is_some());
        if self.active != state.active {
            self.as_mut().rust_mut().active = state.active;
            self.as_mut().active_changed();
        }
        self.as_mut().set_snapshot_generated_at(qs(&state
            .snapshot_generated_at
            .map(|date| date.to_rfc3339())
            .unwrap_or_default()));
        self.as_mut()
            .set_snapshot_status(qs(&state.snapshot_status_text(chrono::Utc::now())));
        self.as_mut().set_snapshot_stale(
            state.snapshot_generated_at.is_some()
                && !state.snapshot_is_fresh_at(chrono::Utc::now()),
        );
    }
}

fn map_row(
    entry: &ContainerFilesystemEntry,
    state: &ContainerFilesControllerState,
) -> ContainerFileRow {
    let mount = state.mount_for_entry(entry);
    let exact_mount = matches!(entry.origin, ContainerFilesystemOrigin::MountOverlay { .. });
    let (mount_kind, mount_source, mount_destination, mount_read_only, mount_action) =
        if let Some(mount) = mount {
            let kind = match &mount.kind {
                ContainerMountOverlayKind::Volume => "volume",
                ContainerMountOverlayKind::Bind => "bind",
                ContainerMountOverlayKind::Tmpfs => "tmpfs",
                ContainerMountOverlayKind::Other(value) => value,
            };
            let action = if exact_mount { kind } else { "" };
            (
                kind.to_string(),
                mount.source.clone().unwrap_or_default(),
                mount.destination.clone(),
                mount.read_only,
                action.to_string(),
            )
        } else {
            (
                String::new(),
                String::new(),
                String::new(),
                false,
                String::new(),
            )
        };
    let origin = match entry.origin {
        ContainerFilesystemOrigin::RootFilesystem => "rootfs",
        ContainerFilesystemOrigin::SyntheticParent => "synthetic",
        ContainerFilesystemOrigin::MountRoute { .. } => "mount_route",
        ContainerFilesystemOrigin::MountOverlay { .. } => "mount_overlay",
        ContainerFilesystemOrigin::ShadowedByMount { .. } => "shadowed",
    };
    ContainerFileRow {
        name: entry.display_name.clone(),
        path: entry.logical_path.clone(),
        entry_type: entry_type_name(entry.entry_type).into(),
        icon_name: if exact_mount {
            match mount_kind.as_str() {
                "volume" => "drive-harddisk",
                "bind" => "folder-home",
                "tmpfs" => "media-flash-memory-stick",
                _ => "folder-network",
            }
        } else {
            match entry.entry_type {
                tuxstack_docker_core::ContainerFilesystemEntryType::Directory => "folder",
                tuxstack_docker_core::ContainerFilesystemEntryType::Symlink
                | tuxstack_docker_core::ContainerFilesystemEntryType::Hardlink => {
                    "emblem-symbolic-link"
                }
                _ => "text-x-generic",
            }
        }
        .into(),
        size_bytes: entry.size.min(i64::MAX as u64) as i64,
        size_text: if entry.entry_type
            == tuxstack_docker_core::ContainerFilesystemEntryType::Directory
        {
            String::new()
        } else {
            format_bytes(entry.size)
        },
        modified_text: entry
            .mtime
            .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        mode_text: format!("{:04o}", entry.mode & 0o7777),
        owner_text: format!("{}:{}", entry.uid, entry.gid),
        link_target: entry.link_target.clone().unwrap_or_default(),
        origin: origin.into(),
        mount_kind,
        mount_source,
        mount_destination,
        mount_read_only,
        mount_action,
    }
}

fn map_files_error(error: &ContainerFilesystemError) -> (&'static str, String) {
    match error {
        ContainerFilesystemError::Cancelled => (
            "cancelled",
            "The filesystem operation was cancelled.".into(),
        ),
        ContainerFilesystemError::Timeout => {
            ("timeout", "The filesystem operation timed out.".into())
        }
        ContainerFilesystemError::Docker(_) => (
            "docker",
            "Docker could not read the container filesystem.".into(),
        ),
        ContainerFilesystemError::InvalidPath { .. } => {
            ("invalid_path", "The container path is invalid.".into())
        }
        ContainerFilesystemError::Io { message, .. } => {
            ("io", format!("Could not save the file: {message}"))
        }
        _ => ("snapshot", error.to_string()),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn local_destination(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("A save destination is required.".into());
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        let encoded = if rest.starts_with('/') {
            rest
        } else if let Some(rest) = rest.strip_prefix("localhost/") {
            return decoded_destination(&format!("/{rest}"));
        } else {
            return Err("The destination must be a local file URL.".into());
        };
        percent_decode_destination(encoded)?
    } else if let Some(encoded) = value.strip_prefix("file:") {
        percent_decode_destination(encoded)?
    } else if value.contains("://") {
        return Err("The destination must be a local file path.".into());
    } else {
        value.to_string()
    };
    if path.contains('\0') {
        return Err("The destination is invalid.".into());
    }
    Ok(PathBuf::from(path))
}

fn decoded_destination(value: &str) -> Result<PathBuf, String> {
    let path = percent_decode_destination(value)?;
    if path.contains('\0') {
        Err("The destination is invalid.".into())
    } else {
        Ok(PathBuf::from(path))
    }
}

fn percent_decode_destination(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("The destination contains an invalid URL escape.".into());
            }
            let (Some(high), Some(low)) = (
                destination_hex(bytes[index + 1]),
                destination_hex(bytes[index + 2]),
            ) else {
                return Err("The destination contains an invalid URL escape.".into());
            };
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "The destination is not valid UTF-8.".to_string())
}

fn destination_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn push_property(list: &mut QList<QVariant>, label: &str, value: &str) {
    let json = serde_json::json!({ "label": label, "value": value });
    // QML can consume JSON strings without a custom shared model type. The
    // dedicated dialog accepts either maps or these compact strings.
    list.append(QVariant::from(&qs(&json.to_string())));
}

fn cancel(slot: &mut Option<CancellationToken>) {
    if let Some(token) = slot.take() {
        token.cancel();
    }
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&qs(value))
}

fn qs(value: &str) -> QString {
    QString::from(value)
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_file_destinations_decode_local_urls_and_reject_remote_urls() {
        assert_eq!(
            local_destination("file:///tmp/My%20Container%20File.txt").unwrap(),
            PathBuf::from("/tmp/My Container File.txt")
        );
        assert_eq!(
            local_destination("file://localhost/tmp/file.txt").unwrap(),
            PathBuf::from("/tmp/file.txt")
        );
        assert!(local_destination("file://example.com/tmp/file.txt").is_err());
        assert!(local_destination("file:///tmp/bad%2").is_err());
    }
}
