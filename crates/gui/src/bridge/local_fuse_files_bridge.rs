//! CXX-Qt bridge for the single local FUSE file browser.
//!
//! Registration contract for the central integration owner:
//!
//! 1. Add `tuxstack-client` and `tuxstack-protocol` to `crates/gui/Cargo.toml`.
//! 2. Export this file from `bridge/mod.rs`, the controller from
//!    `controllers/mod.rs`, and the row mapper from `models/mod.rs`.
//! 3. Add this file to `CxxQtBuilder::files` in `crates/gui/build.rs`.
//! 4. Register `LocalFuseFilesView.qml`, `LocalFuseFilePreviewDialog.qml`, and
//!    `LocalFuseFilePropertiesDialog.qml` in the QML module.
//! 5. Replace Main.qml's three legacy file models with three instances of
//!    `LocalFuseFileListModel` (one per simultaneously retained detail page),
//!    or route all detail pages through one instance if only one can be alive.
//!    Container/Image/Volume wrappers all consume the same API.
//! 6. Connect `openLocalUrl(url)` to QML `Qt.openUrlExternally(url)`. Do not
//!    add an `xdg-open` process bridge.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;
use tuxstack_client::{Client, ClientConfig, ClientError};
use tuxstack_protocol::{
    ConsistencyMode, DockerConnectionStatus, DockerResourceRef, MountAction, MountState,
    ProtocolError, ProtocolErrorCode, ProviderCapabilities, ProviderDescriptor, ProviderKind,
    ProviderStatus, Request, ResourceOperation, ResourcePath, Response,
};

use crate::controllers::local_fuse_files::{
    LocalFileSortColumn, LocalFuseFilesController, LocalFuseFilesState, LocalFuseResourcePath,
    LocalFuseResourceRef, copy_local_file_atomic_cancellable, preview_local_file,
    read_local_directory,
};
use crate::models::local_fuse_file_model::{LocalFuseFileRow, format_bytes, map_local_fuse_row};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

const ROLE_NAME: i32 = 257;
const ROLE_PATH_TOKEN: i32 = 258;
const ROLE_DISPLAY_PATH: i32 = 259;
const ROLE_ENTRY_TYPE: i32 = 260;
const ROLE_ICON_NAME: i32 = 261;
const ROLE_SIZE_BYTES: i32 = 262;
const ROLE_SIZE_TEXT: i32 = 263;
const ROLE_MODIFIED_TEXT: i32 = 264;
const ROLE_KIND_TEXT: i32 = 265;
const ROLE_HIDDEN: i32 = 266;
const ROLE_READABLE: i32 = 267;
const ROLE_SYMLINK_TARGET: i32 = 268;
const ROLE_MODE_TEXT: i32 = 269;
const ROLE_OWNER_TEXT: i32 = 270;
const ROLE_SELECTED: i32 = 271;

#[derive(Default)]
pub struct LocalFuseFileListModelRust {
    pub(crate) controller: LocalFuseFilesController,
    pub(crate) client: Option<Client>,
    pub(crate) rows: Vec<LocalFuseFileRow>,

    pub(crate) files_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) resource_kind: QString,
    pub(crate) resource_id: QString,
    pub(crate) root_path: QString,
    pub(crate) current_path: QString,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_up: bool,
    pub(crate) show_hidden: bool,
    pub(crate) search_query: QString,
    pub(crate) sort_column: QString,
    pub(crate) sort_descending: bool,
    pub(crate) selected_entry_path: QString,
    pub(crate) loading: bool,
    pub(crate) count: i32,
    pub(crate) breadcrumb_model: QVariantList,
    pub(crate) active: bool,

    pub(crate) provider_kind: QString,
    pub(crate) provider_title: QString,
    pub(crate) consistency: QString,
    pub(crate) consistency_detail: QString,
    pub(crate) provider_status: QString,
    pub(crate) provider_status_detail: QString,
    pub(crate) provider_source: QString,
    pub(crate) refresh_action_text: QString,
    pub(crate) can_refresh_provider: bool,
    pub(crate) named_volume: QString,
    pub(crate) host_folder: QString,

    pub(crate) preview_loading: bool,
    pub(crate) preview_name: QString,
    pub(crate) preview_path: QString,
    pub(crate) preview_text: QString,
    pub(crate) preview_mime: QString,
    pub(crate) preview_size_text: QString,
    pub(crate) preview_truncated: bool,
    pub(crate) preview_binary: bool,
    pub(crate) preview_error: QString,

    pub(crate) save_in_progress: bool,
    pub(crate) save_error: QString,
    pub(crate) properties_model: QVariantList,

    pub(crate) resolve_cancel: Option<CancellationToken>,
    pub(crate) list_cancel: Option<CancellationToken>,
    pub(crate) preview_cancel: Option<CancellationToken>,
    pub(crate) save_cancel: Option<CancellationToken>,
    pub(crate) operation_cancel: Option<CancellationToken>,
    pub(crate) preview_generation: u64,
    pub(crate) save_generation: u64,
    pub(crate) operation_generation: u64,
}

impl LocalFuseFileListModelRust {
    fn cancel_all(&mut self) {
        cancel(&mut self.resolve_cancel);
        cancel(&mut self.list_cancel);
        cancel(&mut self.preview_cancel);
        cancel(&mut self.save_cancel);
        cancel(&mut self.operation_cancel);
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

    impl cxx_qt::Threading for LocalFuseFileListModel {}

    unsafe extern "RustQt" {
        /// One read-only local FUSE browser for container, image, and volume refs.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, files_state, cxx_name = "filesState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, resource_kind, cxx_name = "resourceKind")]
        #[qproperty(QString, resource_id, cxx_name = "resourceId")]
        #[qproperty(QString, root_path, cxx_name = "rootPath")]
        #[qproperty(QString, current_path, cxx_name = "currentPath")]
        #[qproperty(bool, can_go_back, cxx_name = "canGoBack")]
        #[qproperty(bool, can_go_up, cxx_name = "canGoUp")]
        #[qproperty(bool, show_hidden, cxx_name = "showHidden", READ, NOTIFY)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn")]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending")]
        #[qproperty(QString, selected_entry_path, cxx_name = "selectedEntryPath")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(QList_QVariant, breadcrumb_model, cxx_name = "breadcrumbModel")]
        #[qproperty(bool, active, READ, NOTIFY)]
        #[qproperty(QString, provider_kind, cxx_name = "providerKind")]
        #[qproperty(QString, provider_title, cxx_name = "providerTitle")]
        #[qproperty(QString, consistency)]
        #[qproperty(QString, consistency_detail, cxx_name = "consistencyDetail")]
        #[qproperty(QString, provider_status, cxx_name = "providerStatus")]
        #[qproperty(QString, provider_status_detail, cxx_name = "providerStatusDetail")]
        #[qproperty(QString, provider_source, cxx_name = "providerSource")]
        #[qproperty(QString, refresh_action_text, cxx_name = "refreshActionText")]
        #[qproperty(bool, can_refresh_provider, cxx_name = "canRefreshProvider")]
        #[qproperty(QString, named_volume, cxx_name = "namedVolume")]
        #[qproperty(QString, host_folder, cxx_name = "hostFolder")]
        #[qproperty(bool, preview_loading, cxx_name = "previewLoading")]
        #[qproperty(QString, preview_name, cxx_name = "previewName")]
        #[qproperty(QString, preview_path, cxx_name = "previewPath")]
        #[qproperty(QString, preview_text, cxx_name = "previewText")]
        #[qproperty(QString, preview_mime, cxx_name = "previewMime")]
        #[qproperty(QString, preview_size_text, cxx_name = "previewSizeText")]
        #[qproperty(bool, preview_truncated, cxx_name = "previewTruncated")]
        #[qproperty(bool, preview_binary, cxx_name = "previewBinary")]
        #[qproperty(QString, preview_error, cxx_name = "previewError")]
        #[qproperty(bool, save_in_progress, cxx_name = "saveInProgress")]
        #[qproperty(QString, save_error, cxx_name = "saveError")]
        #[qproperty(QList_QVariant, properties_model, cxx_name = "propertiesModel")]
        type LocalFuseFileListModel = super::LocalFuseFileListModelRust;

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
        #[cxx_name = "openLocalUrl"]
        fn open_local_url(self: Pin<&mut Self>, url: QString);
        #[qsignal]
        #[cxx_name = "volumeRequested"]
        fn volume_requested(self: Pin<&mut Self>, volume_name: QString);
        #[qsignal]
        #[cxx_name = "startServiceRequested"]
        fn start_service_requested(self: Pin<&mut Self>);
        #[qsignal]
        #[cxx_name = "serviceLogsRequested"]
        fn service_logs_requested(self: Pin<&mut Self>);
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
        #[qsignal]
        #[cxx_name = "notificationRequested"]
        fn notification_requested(self: Pin<&mut Self>, message: QString);

        #[qinvokable]
        #[rust_name = "update_active"]
        #[cxx_name = "setActive"]
        fn set_active(self: Pin<&mut Self>, active: bool);
        #[qinvokable]
        #[cxx_name = "openContainer"]
        fn open_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "openImage"]
        fn open_image(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "openVolume"]
        fn open_volume(self: Pin<&mut Self>, name: &QString);
        #[qinvokable]
        #[cxx_name = "selectContainer"]
        fn select_container(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "clearSelection"]
        fn clear_selection(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "closeResource"]
        fn close_resource(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "closeImage"]
        fn close_image(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "closeVolume"]
        fn close_volume(self: Pin<&mut Self>);
        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshSnapshot"]
        fn refresh_snapshot(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "invalidateSnapshot"]
        fn invalidate_snapshot(self: Pin<&mut Self>, id: &QString);
        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32);
        #[qinvokable]
        #[cxx_name = "retry"]
        fn retry(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "mountFilesystem"]
        fn mount_filesystem(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshProvider"]
        fn refresh_provider(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "openEntry"]
        fn open_entry(self: Pin<&mut Self>, token: &QString);
        #[qinvokable]
        #[cxx_name = "selectEntry"]
        fn select_entry(self: Pin<&mut Self>, token: &QString);
        #[qinvokable]
        #[cxx_name = "goBack"]
        fn go_back(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "goUp"]
        fn go_up(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "navigateTo"]
        fn navigate_to(self: Pin<&mut Self>, token: &QString);
        #[qinvokable]
        #[rust_name = "update_show_hidden"]
        #[cxx_name = "setShowHidden"]
        fn set_show_hidden(self: Pin<&mut Self>, show: bool);
        #[qinvokable]
        #[rust_name = "update_search"]
        #[cxx_name = "setSearchQuery"]
        fn set_search(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        #[cxx_name = "toggleSort"]
        fn toggle_sort(self: Pin<&mut Self>, column: &QString);
        #[qinvokable]
        #[cxx_name = "previewEntry"]
        fn preview_entry(self: Pin<&mut Self>, token: &QString);
        #[qinvokable]
        #[cxx_name = "cancelPreview"]
        fn cancel_preview(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "saveEntry"]
        fn save_entry(self: Pin<&mut Self>, token: &QString, destination: &QString);
        #[qinvokable]
        #[cxx_name = "cancelSave"]
        fn cancel_save(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "loadProperties"]
        fn load_properties(self: Pin<&mut Self>, token: &QString);
        #[qinvokable]
        #[cxx_name = "openInFileManager"]
        fn open_in_file_manager(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "openHostFolder"]
        fn open_host_folder(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "openInVolumes"]
        fn open_in_volumes(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "requestStartService"]
        fn request_start_service(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "requestServiceLogs"]
        fn request_service_logs(self: Pin<&mut Self>);
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }
}

impl qobject::LocalFuseFileListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            ROLE_NAME => qv(&row.name),
            ROLE_PATH_TOKEN => qv(&row.path_token),
            ROLE_DISPLAY_PATH => qv(&row.display_path),
            ROLE_ENTRY_TYPE => qv(&row.entry_type),
            ROLE_ICON_NAME => qv(&row.icon_name),
            ROLE_SIZE_BYTES => QVariant::from(&row.size_bytes),
            ROLE_SIZE_TEXT => qv(&row.size_text),
            ROLE_MODIFIED_TEXT => qv(&row.modified_text),
            ROLE_KIND_TEXT => qv(&row.kind_text),
            ROLE_HIDDEN => QVariant::from(&row.hidden),
            ROLE_READABLE => QVariant::from(&row.readable),
            ROLE_SYMLINK_TARGET => qv(&row.symlink_target),
            ROLE_MODE_TEXT => qv(&row.mode_text),
            ROLE_OWNER_TEXT => qv(&row.owner_text),
            ROLE_SELECTED => QVariant::from(
                &(self.controller.selected_token.as_deref() == Some(row.path_token.as_str())),
            ),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (ROLE_NAME, "name"),
            (ROLE_PATH_TOKEN, "pathToken"),
            (ROLE_DISPLAY_PATH, "displayPath"),
            (ROLE_ENTRY_TYPE, "entryType"),
            (ROLE_ICON_NAME, "iconName"),
            (ROLE_SIZE_BYTES, "sizeBytes"),
            (ROLE_SIZE_TEXT, "sizeText"),
            (ROLE_MODIFIED_TEXT, "modifiedText"),
            (ROLE_KIND_TEXT, "kindText"),
            (ROLE_HIDDEN, "hidden"),
            (ROLE_READABLE, "readable"),
            (ROLE_SYMLINK_TARGET, "symlinkTarget"),
            (ROLE_MODE_TEXT, "modeText"),
            (ROLE_OWNER_TEXT, "ownerText"),
            (ROLE_SELECTED, "selected"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    pub(crate) fn update_active(mut self: Pin<&mut Self>, active: bool) {
        self.as_mut().rust_mut().controller.set_active(active);
        if self.active != active {
            self.as_mut().rust_mut().active = active;
            self.as_mut().active_changed();
        }
        if active && self.controller.resource.is_some() && self.controller.resource_path.is_none() {
            self.as_mut().resolve_resource();
        }
    }

    pub(crate) fn open_container(self: Pin<&mut Self>, id: &QString) {
        self.open_resource(LocalFuseResourceRef::Container(id.to_string()));
    }

    pub(crate) fn open_image(self: Pin<&mut Self>, id: &QString) {
        self.open_resource(LocalFuseResourceRef::Image(id.to_string()));
    }

    pub(crate) fn open_volume(self: Pin<&mut Self>, name: &QString) {
        self.open_resource(LocalFuseResourceRef::Volume(name.to_string()));
    }

    pub(crate) fn select_container(self: Pin<&mut Self>, id: &QString) {
        self.open_resource(LocalFuseResourceRef::Container(id.to_string()));
    }

    pub(crate) fn clear_selection(self: Pin<&mut Self>) {
        self.close_resource();
    }

    pub(crate) fn close_resource(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().cancel_all();
        self.as_mut().rust_mut().controller.clear_resource();
        self.as_mut().clear_provider();
        self.as_mut().clear_preview();
        self.as_mut().publish_state();
    }

    pub(crate) fn close_image(self: Pin<&mut Self>) {
        self.close_resource();
    }

    pub(crate) fn close_volume(self: Pin<&mut Self>) {
        self.close_resource();
    }

    pub(crate) fn refresh(mut self: Pin<&mut Self>) {
        if self.controller.resource_path.is_some() {
            self.as_mut().query_descriptor_then_list();
        } else {
            self.as_mut().resolve_resource();
        }
    }

    pub(crate) fn refresh_snapshot(self: Pin<&mut Self>) {
        self.refresh_provider();
    }

    pub(crate) fn invalidate_snapshot(mut self: Pin<&mut Self>, id: &QString) {
        if matches!(
            &self.controller.resource,
            Some(LocalFuseResourceRef::Container(current)) if current == &id.to_string()
        ) {
            self.as_mut().resolve_resource();
        }
    }

    pub(crate) fn set_connection_state(mut self: Pin<&mut Self>, docker_status: i32) {
        // Legacy Main.qml forwards its Docker status here. The daemon remains
        // authoritative, so a connected value never marks the browser ready;
        // a disconnected value merely triggers a fresh daemon status query.
        if docker_status != 1 && self.controller.active {
            self.as_mut().resolve_resource();
        }
    }

    pub(crate) fn retry(self: Pin<&mut Self>) {
        self.resolve_resource();
    }

    pub(crate) fn mount_filesystem(self: Pin<&mut Self>) {
        self.perform_mount();
    }

    pub(crate) fn refresh_provider(self: Pin<&mut Self>) {
        self.perform_provider_refresh();
    }

    pub(crate) fn open_entry(mut self: Pin<&mut Self>, token: &QString) {
        let token = token.to_string();
        let Some(entry) = self.controller.entry(&token).cloned() else {
            return;
        };
        if entry.kind.is_directory() {
            if self
                .as_mut()
                .rust_mut()
                .controller
                .navigate_to_token(&token, true)
            {
                self.as_mut().query_descriptor_then_list();
            }
        } else if entry.kind == crate::controllers::local_fuse_files::LocalFileKind::Symlink {
            self.as_mut().open_symlink(token);
        } else if entry.kind.is_previewable() {
            self.as_mut().preview_entry(&qstring(&token));
        } else {
            self.as_mut().notification_requested(qstring(
                "This special filesystem node cannot be opened from the read-only browser.",
            ));
        }
    }

    pub(crate) fn select_entry(mut self: Pin<&mut Self>, token: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .select_token(Some(&token.to_string()));
        self.as_mut().publish_state();
    }

    pub(crate) fn go_back(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().controller.go_back() {
            self.as_mut().query_descriptor_then_list();
        }
    }

    pub(crate) fn go_up(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().controller.go_up() {
            self.as_mut().query_descriptor_then_list();
        }
    }

    pub(crate) fn navigate_to(mut self: Pin<&mut Self>, token: &QString) {
        if self
            .as_mut()
            .rust_mut()
            .controller
            .navigate_to_token(&token.to_string(), true)
        {
            self.as_mut().query_descriptor_then_list();
        }
    }

    pub(crate) fn update_show_hidden(mut self: Pin<&mut Self>, show: bool) {
        self.as_mut().rust_mut().controller.set_show_hidden(show);
        self.as_mut().publish_state();
    }

    pub(crate) fn update_search(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .set_search(&query.to_string());
        self.as_mut().publish_state();
    }

    pub(crate) fn toggle_sort(mut self: Pin<&mut Self>, column: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .toggle_sort(LocalFileSortColumn::parse(&column.to_string()));
        self.as_mut().publish_state();
    }

    pub(crate) fn preview_entry(mut self: Pin<&mut Self>, token: &QString) {
        let token = token.to_string();
        let Some(entry) = self.controller.entry(&token).cloned() else {
            return;
        };
        self.as_mut()
            .rust_mut()
            .controller
            .select_token(Some(&token));
        self.as_mut().publish_state();
        if !entry.kind.is_previewable() {
            self.as_mut()
                .preview_failed(qstring("Only regular files can be previewed."));
            return;
        }
        let Some(path) = self.controller.local_path_for_token(&token) else {
            return;
        };
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        self.as_mut().rust_mut().preview_generation = self.preview_generation.wrapping_add(1);
        let generation = self.preview_generation;
        self.as_mut().rust_mut().preview_loading = true;
        self.as_mut().rust_mut().preview_name = qstring(&entry.display_name);
        self.as_mut().rust_mut().preview_path = qstring(&entry.display_path);
        self.as_mut().rust_mut().preview_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().preview_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                value = preview_local_file(path, &entry.name_raw) => value,
            };
            qt.queue(move |mut model| {
                if model.preview_generation != generation {
                    return;
                }
                model.as_mut().rust_mut().preview_loading = false;
                match result {
                    Ok(preview) => {
                        model.as_mut().rust_mut().preview_size_text =
                            qstring(&format_bytes(preview.file_size));
                        model.as_mut().rust_mut().preview_mime = qstring(&preview.mime_hint);
                        model.as_mut().rust_mut().preview_truncated = preview.truncated;
                        match String::from_utf8(preview.bytes) {
                            Ok(text) if !text.contains('\0') => {
                                model.as_mut().rust_mut().preview_binary = false;
                                model.as_mut().rust_mut().preview_text = qstring(&text);
                            }
                            _ => {
                                model.as_mut().rust_mut().preview_binary = true;
                                model.as_mut().rust_mut().preview_text = QString::default();
                            }
                        }
                        model.as_mut().preview_ready();
                    }
                    Err(error) => {
                        let message = local_io_message(&error, "preview the file");
                        model.as_mut().rust_mut().preview_error = qstring(&message);
                        model.as_mut().preview_failed(qstring(&message));
                    }
                }
            })
            .ok();
        });
    }

    pub(crate) fn cancel_preview(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        self.as_mut().rust_mut().preview_generation = self.preview_generation.wrapping_add(1);
        self.as_mut().clear_preview();
    }

    pub(crate) fn save_entry(mut self: Pin<&mut Self>, token: &QString, destination: &QString) {
        let token = token.to_string();
        let Some(entry) = self.controller.entry(&token) else {
            return;
        };
        if !entry.kind.is_previewable() {
            self.as_mut()
                .save_failed(qstring("Only regular files can be saved."));
            return;
        }
        let Some(source) = self.controller.local_path_for_token(&token) else {
            return;
        };
        let destination = match local_destination(&destination.to_string()) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut().save_failed(qstring(&message));
                return;
            }
        };
        cancel(&mut self.as_mut().rust_mut().save_cancel);
        self.as_mut().rust_mut().save_generation = self.save_generation.wrapping_add(1);
        let generation = self.save_generation;
        self.as_mut().rust_mut().save_in_progress = true;
        self.as_mut().rust_mut().save_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().save_cancel = Some(token.clone());
        let shown_destination = destination.display().to_string();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = copy_local_file_atomic_cancellable(source, destination, token).await;
            qt.queue(move |mut model| {
                if model.save_generation != generation {
                    return;
                }
                model.as_mut().rust_mut().save_in_progress = false;
                match result {
                    Ok(_) => model.as_mut().save_completed(qstring(&shown_destination)),
                    Err(error) => {
                        let message = local_io_message(&error, "save the file");
                        model.as_mut().rust_mut().save_error = qstring(&message);
                        model.as_mut().save_failed(qstring(&message));
                    }
                }
            })
            .ok();
        });
    }

    pub(crate) fn cancel_save(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().save_cancel);
        self.as_mut().rust_mut().save_generation = self.save_generation.wrapping_add(1);
        self.as_mut().rust_mut().save_in_progress = false;
    }

    pub(crate) fn load_properties(mut self: Pin<&mut Self>, token: &QString) {
        let token = token.to_string();
        let Some(row) = self
            .rows
            .iter()
            .find(|row| row.path_token == token)
            .cloned()
        else {
            return;
        };
        let mut properties = QVariantList::default();
        push_property(&mut properties, "Name", &row.name);
        push_property(&mut properties, "Path", &row.display_path);
        push_property(&mut properties, "Kind", &row.kind_text);
        push_property(&mut properties, "Size", &row.size_text);
        push_property(&mut properties, "Modified", &row.modified_text);
        push_property(&mut properties, "Permissions", &row.mode_text);
        push_property(&mut properties, "Owner", &row.owner_text);
        if !row.symlink_target.is_empty() {
            push_property(&mut properties, "Symlink Target", &row.symlink_target);
        }
        push_property(
            &mut properties,
            "Provider",
            &self.provider_title.to_string(),
        );
        push_property(
            &mut properties,
            "Consistency",
            &self.consistency.to_string(),
        );
        self.as_mut().set_properties_model(properties);
        self.as_mut().properties_ready();
    }

    pub(crate) fn open_in_file_manager(mut self: Pin<&mut Self>) {
        let Some(path) = self
            .controller
            .local_path_for_components(&self.controller.current_components)
        else {
            return;
        };
        self.as_mut().open_local_url(qstring(&file_url(&path)));
    }

    pub(crate) fn open_host_folder(mut self: Pin<&mut Self>) {
        let source = self.host_folder.to_string();
        if source.is_empty() {
            return;
        }
        let path = PathBuf::from(source);
        if path.is_absolute() {
            self.as_mut().open_local_url(qstring(&file_url(&path)));
        }
    }

    pub(crate) fn open_in_volumes(mut self: Pin<&mut Self>) {
        let volume = self.named_volume.to_string();
        if !volume.is_empty() {
            self.as_mut().volume_requested(qstring(&volume));
        }
    }

    pub(crate) fn request_start_service(mut self: Pin<&mut Self>) {
        self.as_mut().start_service_requested();
    }

    pub(crate) fn request_service_logs(mut self: Pin<&mut Self>) {
        self.as_mut().service_logs_requested();
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().cancel_all();
        self.as_mut().rust_mut().client = None;
        self.as_mut().rust_mut().controller.clear_resource();
        self.as_mut().publish_state();
    }

    fn open_resource(mut self: Pin<&mut Self>, resource: LocalFuseResourceRef) {
        if resource.id().trim().is_empty() {
            self.as_mut().close_resource();
            return;
        }
        let changed = self
            .as_mut()
            .rust_mut()
            .controller
            .select_resource(resource);
        if !changed {
            // Detail models emit several NOTIFY signals while one inspect is
            // applied. Re-opening the same resource for every signal cancels
            // the current FUSE lookup and starts it again, which makes the
            // third column flash continuously. Resource resolution is only
            // needed when the resource identity actually changes; refreshes
            // are explicit and handled by refresh_provider().
            return;
        }
        self.as_mut().rust_mut().cancel_all();
        self.as_mut().clear_provider();
        self.as_mut().clear_preview();
        if self.controller.active {
            self.as_mut().resolve_resource();
        } else {
            self.as_mut().publish_state();
        }
    }

    fn open_symlink(mut self: Pin<&mut Self>, path_token: String) {
        let Some(path) = self.controller.local_path_for_token(&path_token) else {
            return;
        };
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        let cancellation = CancellationToken::new();
        self.as_mut().rust_mut().preview_cancel = Some(cancellation.clone());
        let resource_generation = self.controller.generation;
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            // `metadata` follows the link through FUSE. The daemon/FUSE layer
            // owns link rewriting and escape prevention; GUI never resolves or
            // joins the raw target returned by readlink.
            let metadata = tokio::select! {
                _ = cancellation.cancelled() => return,
                result = tokio::fs::metadata(path) => result,
            };
            qt.queue(move |mut model| {
                if model.controller.generation != resource_generation {
                    return;
                }
                match metadata {
                    Ok(value) if value.is_dir() => {
                        if model
                            .as_mut()
                            .rust_mut()
                            .controller
                            .navigate_to_token(&path_token, true)
                        {
                            model.as_mut().query_descriptor_then_list();
                        }
                    }
                    Ok(value) if value.is_file() => {
                        model.as_mut().preview_entry(&qstring(&path_token));
                    }
                    Ok(_) => model.as_mut().notification_requested(qstring(
                        "This symlink resolves to a special node that cannot be opened.",
                    )),
                    Err(error) => {
                        let message = local_io_message(&error, "open this symbolic link");
                        model.as_mut().preview_failed(qstring(&message));
                    }
                }
            })
            .ok();
        });
    }

    fn resolve_resource(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().resolve_cancel);
        let Some((generation, resource)) = self.as_mut().rust_mut().controller.begin_resolve()
        else {
            self.as_mut().publish_state();
            return;
        };
        self.as_mut().publish_state();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().resolve_cancel = Some(token.clone());
        let existing_client = self.client.clone();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = resolve_over_ipc(existing_client, resource).await;
            if token.is_cancelled() {
                return;
            }
            qt.queue(move |mut model| match result {
                Ok((client, path, descriptor)) => {
                    model.as_mut().rust_mut().client = Some(client);
                    if !model
                        .as_mut()
                        .rust_mut()
                        .controller
                        .apply_resource_path(generation, LocalFuseResourcePath { root: path })
                    {
                        return;
                    }
                    model.as_mut().apply_descriptor(&descriptor);
                    if descriptor_is_ready(&descriptor) {
                        model.as_mut().load_directory();
                    } else {
                        model.as_mut().publish_state();
                    }
                }
                Err(error) => {
                    let (state, kind, message) = error.presentation();
                    model
                        .as_mut()
                        .rust_mut()
                        .controller
                        .apply_error(generation, state, kind, message);
                    model.as_mut().publish_state();
                }
            })
            .ok();
        });
    }

    fn query_descriptor_then_list(mut self: Pin<&mut Self>) {
        let Some(client) = self.client.clone() else {
            self.as_mut().resolve_resource();
            return;
        };
        let Some(resource) = self.controller.resource.clone() else {
            return;
        };
        // The wire protocol currently models logical components as UTF-8.
        // Raw non-UTF-8 navigation remains fully supported for local I/O; in
        // that case retain the last daemon descriptor rather than guessing it.
        let components = self
            .controller
            .current_components
            .iter()
            .map(|value| String::from_utf8(value.clone()))
            .collect::<Result<Vec<_>, _>>();
        let Ok(components) = components else {
            self.as_mut().load_directory();
            return;
        };
        cancel(&mut self.as_mut().rust_mut().resolve_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().resolve_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let wire_resource = wire_resource(&resource);
            let response = client
                .request(Request::GetProviderDescriptor(ResourcePath {
                    resource: wire_resource,
                    components,
                }))
                .await;
            if token.is_cancelled() {
                return;
            }
            qt.queue(move |mut model| {
                match response {
                    Ok(Response::ProviderDescriptor(descriptor)) => {
                        model.as_mut().apply_descriptor(&descriptor);
                        if !descriptor_is_ready(&descriptor) {
                            model.as_mut().publish_state();
                            return;
                        }
                    }
                    Ok(Response::Error(error)) => {
                        let (state, kind) = protocol_error_state(&error);
                        let generation = model.controller.generation;
                        model.as_mut().rust_mut().controller.apply_error(
                            generation,
                            state,
                            kind,
                            error.message,
                        );
                        model.as_mut().publish_state();
                        return;
                    }
                    Err(error) => {
                        let generation = model.controller.generation;
                        model.as_mut().rust_mut().controller.apply_error(
                            generation,
                            LocalFuseFilesState::DaemonOffline,
                            "daemon_offline",
                            daemon_error_message(&error),
                        );
                        model.as_mut().publish_state();
                        return;
                    }
                    _ => {}
                }
                model.as_mut().load_directory();
            })
            .ok();
        });
    }

    fn load_directory(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let Some((generation, root, components)) = self.as_mut().rust_mut().controller.begin_list()
        else {
            self.as_mut().publish_state();
            return;
        };
        self.as_mut().publish_state();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().list_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                value = read_local_directory(root, components) => value,
            };
            qt.queue(move |mut model| {
                match result {
                    Ok(entries) => {
                        model
                            .as_mut()
                            .rust_mut()
                            .controller
                            .apply_entries(generation, entries);
                    }
                    Err(error) => {
                        let (state, kind) = local_io_state(&error);
                        let message = local_io_message(&error, "read this FUSE folder");
                        model
                            .as_mut()
                            .rust_mut()
                            .controller
                            .apply_error(generation, state, kind, message);
                    }
                }
                model.as_mut().publish_state();
            })
            .ok();
        });
    }

    fn perform_mount(mut self: Pin<&mut Self>) {
        self.as_mut()
            .perform_operation(Request::SetMountState(MountAction::Mount), true);
    }

    fn perform_provider_refresh(mut self: Pin<&mut Self>) {
        let Some(resource) = self.controller.resource.clone() else {
            return;
        };
        let operation = match resource {
            LocalFuseResourceRef::Image(_) => ResourceOperation::RebuildIndex {
                resource: wire_resource(&resource),
            },
            _ => ResourceOperation::Refresh {
                path: ResourcePath {
                    resource: wire_resource(&resource),
                    components: Vec::new(),
                },
            },
        };
        self.as_mut()
            .perform_operation(Request::PerformResourceOperation(operation), true);
    }

    fn perform_operation(mut self: Pin<&mut Self>, request: Request, resolve_after: bool) {
        cancel(&mut self.as_mut().rust_mut().operation_cancel);
        self.as_mut().rust_mut().operation_generation = self.operation_generation.wrapping_add(1);
        let generation = self.operation_generation;
        let token = CancellationToken::new();
        self.as_mut().rust_mut().operation_cancel = Some(token.clone());
        let existing_client = self.client.clone();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = async {
                let client = match existing_client {
                    Some(client) if client.is_connected() => client,
                    _ => connect_client().await?,
                };
                let response = client.request(request).await?;
                Ok::<_, ClientError>((client, response))
            }
            .await;
            if token.is_cancelled() {
                return;
            }
            qt.queue(move |mut model| {
                if model.operation_generation != generation {
                    return;
                }
                match result {
                    Ok((client, Response::Error(error))) => {
                        model.as_mut().rust_mut().client = Some(client);
                        let (state, kind) = protocol_error_state(&error);
                        model.as_mut().rust_mut().controller.set_external_state(
                            state,
                            kind,
                            error.message,
                        );
                        model.as_mut().publish_state();
                    }
                    Ok((client, _)) => {
                        model.as_mut().rust_mut().client = Some(client);
                        if resolve_after {
                            model.as_mut().resolve_resource();
                        }
                    }
                    Err(error) => {
                        model.as_mut().rust_mut().controller.set_external_state(
                            LocalFuseFilesState::DaemonOffline,
                            "daemon_offline",
                            daemon_error_message(&error),
                        );
                        model.as_mut().publish_state();
                    }
                }
            })
            .ok();
        });
    }

    fn apply_descriptor(mut self: Pin<&mut Self>, descriptor: &ProviderDescriptor) {
        let presentation = ProviderPresentation::from_descriptor(descriptor);
        self.as_mut().set_provider_kind(qstring(presentation.kind));
        self.as_mut()
            .set_provider_title(qstring(&presentation.title));
        self.as_mut()
            .set_consistency(qstring(&presentation.consistency));
        self.as_mut()
            .set_consistency_detail(qstring(&presentation.consistency_detail));
        self.as_mut()
            .set_provider_status(qstring(presentation.status));
        self.as_mut()
            .set_provider_status_detail(qstring(&presentation.status_detail));
        self.as_mut()
            .set_provider_source(qstring(descriptor.source.as_deref().unwrap_or_default()));
        self.as_mut()
            .set_refresh_action_text(qstring(presentation.refresh_text));
        self.as_mut().set_can_refresh_provider(
            descriptor
                .capabilities
                .contains(ProviderCapabilities::REFRESH)
                || matches!(
                    descriptor.kind,
                    ProviderKind::ContainerRootfsSnapshot | ProviderKind::ImageRootfsImmutable
                ),
        );
        self.as_mut()
            .set_named_volume(qstring(&presentation.named_volume));
        self.as_mut()
            .set_host_folder(qstring(&presentation.host_folder));

        let generation = self.controller.generation;
        match &descriptor.status {
            ProviderStatus::IndexBuilding { .. } => {
                self.as_mut().rust_mut().controller.apply_error(
                    generation,
                    LocalFuseFilesState::IndexBuilding,
                    "index_building",
                    &presentation.status_detail,
                );
            }
            ProviderStatus::SnapshotBuilding { .. } => {
                self.as_mut().rust_mut().controller.apply_error(
                    generation,
                    LocalFuseFilesState::SnapshotBuilding,
                    "snapshot_building",
                    &presentation.status_detail,
                );
            }
            ProviderStatus::Unavailable { reason } => {
                self.as_mut().rust_mut().controller.apply_error(
                    generation,
                    LocalFuseFilesState::ProviderUnavailable,
                    "provider_unavailable",
                    reason,
                );
            }
            ProviderStatus::PermissionDenied { reason } => {
                self.as_mut().rust_mut().controller.apply_error(
                    generation,
                    LocalFuseFilesState::PermissionDenied,
                    "permission_denied",
                    reason,
                );
            }
            ProviderStatus::Ready => {}
            _ => {}
        }
    }

    fn clear_provider(mut self: Pin<&mut Self>) {
        self.as_mut().set_provider_kind(QString::default());
        self.as_mut().set_provider_title(QString::default());
        self.as_mut().set_consistency(QString::default());
        self.as_mut().set_consistency_detail(QString::default());
        self.as_mut().set_provider_status(QString::default());
        self.as_mut().set_provider_status_detail(QString::default());
        self.as_mut().set_provider_source(QString::default());
        self.as_mut().set_refresh_action_text(QString::default());
        self.as_mut().set_can_refresh_provider(false);
        self.as_mut().set_named_volume(QString::default());
        self.as_mut().set_host_folder(QString::default());
    }

    fn clear_preview(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        rust.preview_loading = false;
        rust.preview_name = QString::default();
        rust.preview_path = QString::default();
        rust.preview_text = QString::default();
        rust.preview_mime = QString::default();
        rust.preview_size_text = QString::default();
        rust.preview_truncated = false;
        rust.preview_binary = false;
        rust.preview_error = QString::default();
    }

    fn publish_state(mut self: Pin<&mut Self>) {
        let controller = self.controller.clone();
        let rows = controller
            .visible_entries()
            .into_iter()
            .map(map_local_fuse_row)
            .collect::<Vec<_>>();
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();

        self.as_mut()
            .set_files_state(qstring(controller.state.as_str()));
        self.as_mut()
            .set_error_kind(qstring(&controller.error_kind));
        self.as_mut()
            .set_error_message(qstring(&controller.error_message));
        self.as_mut().set_resource_kind(qstring(
            controller
                .resource
                .as_ref()
                .map_or("", |value| value.kind_name()),
        ));
        self.as_mut().set_resource_id(qstring(
            controller.resource.as_ref().map_or("", |value| value.id()),
        ));
        self.as_mut().set_root_path(qstring(
            &controller
                .resource_path
                .as_ref()
                .map(|value| value.root.display().to_string())
                .unwrap_or_default(),
        ));
        self.as_mut()
            .set_current_path(qstring(&controller.current_display_path()));
        self.as_mut().set_can_go_back(controller.can_go_back());
        self.as_mut().set_can_go_up(controller.can_go_up());
        self.as_mut()
            .set_sort_column(qstring(controller.sort_column.as_str()));
        self.as_mut()
            .set_sort_descending(controller.sort_descending);
        self.as_mut().set_selected_entry_path(qstring(
            controller.selected_token.as_deref().unwrap_or_default(),
        ));
        self.as_mut().set_loading(matches!(
            controller.state,
            LocalFuseFilesState::Resolving
                | LocalFuseFilesState::Loading
                | LocalFuseFilesState::IndexBuilding
                | LocalFuseFilesState::SnapshotBuilding
        ));
        let count = saturating_i32(self.rows.len());
        self.as_mut().set_count(count);
        self.as_mut()
            .set_breadcrumb_model(breadcrumb_list(&controller));
        if self.show_hidden != controller.show_hidden {
            self.as_mut().rust_mut().show_hidden = controller.show_hidden;
            self.as_mut().show_hidden_changed();
        }
        let search = qstring(&controller.search_query);
        if self.search_query != search {
            self.as_mut().rust_mut().search_query = search;
            self.as_mut().search_query_changed();
        }
        if self.active != controller.active {
            self.as_mut().rust_mut().active = controller.active;
            self.as_mut().active_changed();
        }
    }
}

#[derive(Debug)]
enum ResolveError {
    Client(ClientError),
    Protocol(ProtocolError),
    Unexpected,
    DockerUnavailable(String),
    FuseUnavailable(String),
}

impl ResolveError {
    fn presentation(&self) -> (LocalFuseFilesState, &'static str, String) {
        match self {
            Self::Client(error) => (
                LocalFuseFilesState::DaemonOffline,
                "daemon_offline",
                daemon_error_message(error),
            ),
            Self::DockerUnavailable(message) => (
                LocalFuseFilesState::DockerOffline,
                "docker_offline",
                message.clone(),
            ),
            Self::FuseUnavailable(message) => (
                LocalFuseFilesState::FuseOffline,
                "fuse_offline",
                message.clone(),
            ),
            Self::Protocol(error) => {
                let (state, kind) = protocol_error_state(error);
                (state, kind, error.message.clone())
            }
            Self::Unexpected => (
                LocalFuseFilesState::Error,
                "protocol",
                "The TuxStack service returned an unexpected response.".into(),
            ),
        }
    }
}

async fn resolve_over_ipc(
    existing: Option<Client>,
    resource: LocalFuseResourceRef,
) -> Result<(Client, PathBuf, ProviderDescriptor), ResolveError> {
    let client = match existing {
        Some(client) if client.is_connected() => client,
        _ => connect_client().await.map_err(ResolveError::Client)?,
    };
    let status = match client
        .request(Request::GetDaemonStatus)
        .await
        .map_err(ResolveError::Client)?
    {
        Response::DaemonStatus(status) => status,
        Response::Error(error) => return Err(ResolveError::Protocol(error)),
        _ => return Err(ResolveError::Unexpected),
    };
    match status.docker {
        DockerConnectionStatus::Connected { .. } => {}
        DockerConnectionStatus::Reconnecting => {
            return Err(ResolveError::DockerUnavailable(
                "Docker Engine is reconnecting.".into(),
            ));
        }
        DockerConnectionStatus::Unavailable { reason } => {
            return Err(ResolveError::DockerUnavailable(if reason.is_empty() {
                "Docker Engine is unavailable.".into()
            } else {
                reason
            }));
        }
        _ => {
            return Err(ResolveError::DockerUnavailable(
                "Docker Engine is unavailable.".into(),
            ));
        }
    }
    match status.mount.state {
        MountState::Mounted => {}
        MountState::Mounting => {
            return Err(ResolveError::FuseUnavailable(
                "Docker filesystem is mounting.".into(),
            ));
        }
        MountState::Unmounting => {
            return Err(ResolveError::FuseUnavailable(
                "Docker filesystem is unmounting.".into(),
            ));
        }
        MountState::Failed { reason } => {
            return Err(ResolveError::FuseUnavailable(if reason.is_empty() {
                "Docker filesystem is unavailable.".into()
            } else {
                reason
            }));
        }
        MountState::Unmounted => {
            return Err(ResolveError::FuseUnavailable(
                "Docker filesystem is unavailable.".into(),
            ));
        }
        _ => {
            return Err(ResolveError::FuseUnavailable(
                "Docker filesystem is unavailable.".into(),
            ));
        }
    }
    match client
        .request(Request::GetResourceFusePath(wire_resource(&resource)))
        .await
        .map_err(ResolveError::Client)?
    {
        Response::ResourceFusePath(resource_path) => {
            Ok((client, resource_path.path, resource_path.descriptor))
        }
        Response::Error(error) => Err(ResolveError::Protocol(error)),
        _ => Err(ResolveError::Unexpected),
    }
}

async fn connect_client() -> Result<Client, ClientError> {
    let config = ClientConfig::from_env(env!("CARGO_PKG_VERSION"))?;
    Client::connect(config).await
}

fn wire_resource(resource: &LocalFuseResourceRef) -> DockerResourceRef {
    match resource {
        LocalFuseResourceRef::Container(container_id) => DockerResourceRef::Container {
            container_id: container_id.clone(),
        },
        LocalFuseResourceRef::Image(image_id) => DockerResourceRef::Image {
            image_id: image_id.clone(),
        },
        LocalFuseResourceRef::Volume(volume_name) => DockerResourceRef::Volume {
            volume_name: volume_name.clone(),
        },
    }
}

struct ProviderPresentation {
    kind: &'static str,
    title: String,
    consistency: String,
    consistency_detail: String,
    status: &'static str,
    status_detail: String,
    refresh_text: &'static str,
    named_volume: String,
    host_folder: String,
}

impl ProviderPresentation {
    fn from_descriptor(descriptor: &ProviderDescriptor) -> Self {
        let source = descriptor.source.clone().unwrap_or_default();
        let (kind, title, refresh_text) = match descriptor.kind {
            ProviderKind::ContainerRootfsSnapshot => (
                "container_snapshot",
                "Container filesystem".into(),
                "Refresh Snapshot",
            ),
            ProviderKind::ContainerArchiveLive => (
                "container_archive",
                "Container runtime path".into(),
                "Refresh",
            ),
            ProviderKind::NamedVolumeLive => (
                "named_volume",
                if source.is_empty() {
                    "Named volume".into()
                } else {
                    format!("Volume: {source}")
                },
                "Refresh",
            ),
            ProviderKind::LocalBindLive => ("local_bind", "Bind mount".into(), "Refresh"),
            ProviderKind::HelperBindLive => ("helper_bind", "Bind mount".into(), "Refresh"),
            ProviderKind::TmpfsLive => ("tmpfs", "Tmpfs mount".into(), "Refresh"),
            ProviderKind::RuntimeMount => ("runtime_mount", "Runtime mount".into(), "Refresh"),
            ProviderKind::ImageRootfsImmutable => {
                ("image", "Image filesystem".into(), "Refresh Image Index")
            }
            _ => ("provider", "Docker filesystem".into(), "Refresh"),
        };
        let (consistency, consistency_detail) = match descriptor.consistency {
            ConsistencyMode::Immutable => ("Immutable · Read-only".into(), String::new()),
            ConsistencyMode::Live => ("Live · Read-only".into(), String::new()),
            ConsistencyMode::Snapshot {
                captured_at_unix_ms,
                generation,
            } => {
                let captured =
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(captured_at_unix_ms)
                        .map(|date| date.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "Unknown capture time".into());
                (
                    "Snapshot · Read-only".into(),
                    format!("Captured {captured} · generation {generation}"),
                )
            }
            ConsistencyMode::OperationTimeRead => {
                ("Operation-time read · Read-only".into(), String::new())
            }
            ConsistencyMode::Unavailable => ("Unavailable".into(), String::new()),
            _ => ("Unavailable".into(), String::new()),
        };
        let (status, status_detail) = match &descriptor.status {
            ProviderStatus::Ready => ("ready", String::new()),
            ProviderStatus::IndexBuilding { progress_percent } => (
                "index_building",
                progress_text("Image filesystem index is building", *progress_percent),
            ),
            ProviderStatus::SnapshotBuilding { progress_percent } => (
                "snapshot_building",
                progress_text(
                    "Container filesystem snapshot is building",
                    *progress_percent,
                ),
            ),
            ProviderStatus::Unavailable { reason } => ("unavailable", reason.clone()),
            ProviderStatus::PermissionDenied { reason } => ("permission_denied", reason.clone()),
            _ => (
                "unavailable",
                "The filesystem provider is unavailable.".into(),
            ),
        };
        Self {
            kind,
            title,
            consistency,
            consistency_detail,
            status,
            status_detail,
            refresh_text,
            named_volume: if matches!(descriptor.kind, ProviderKind::NamedVolumeLive) {
                source.clone()
            } else {
                String::new()
            },
            host_folder: if matches!(
                descriptor.kind,
                ProviderKind::LocalBindLive | ProviderKind::HelperBindLive
            ) {
                source
            } else {
                String::new()
            },
        }
    }
}

fn descriptor_is_ready(descriptor: &ProviderDescriptor) -> bool {
    matches!(descriptor.status, ProviderStatus::Ready)
}

fn progress_text(prefix: &str, percent: Option<u8>) -> String {
    percent.map_or_else(
        || format!("{prefix}."),
        |value| format!("{prefix}: {value}%"),
    )
}

fn protocol_error_state(error: &ProtocolError) -> (LocalFuseFilesState, &'static str) {
    match error.code {
        ProtocolErrorCode::DaemonUnavailable => {
            (LocalFuseFilesState::DaemonOffline, "daemon_offline")
        }
        ProtocolErrorCode::DockerUnavailable => {
            (LocalFuseFilesState::DockerOffline, "docker_offline")
        }
        ProtocolErrorCode::FuseUnavailable => (LocalFuseFilesState::FuseOffline, "fuse_offline"),
        ProtocolErrorCode::ProviderUnavailable | ProtocolErrorCode::NotFound => (
            LocalFuseFilesState::ProviderUnavailable,
            "provider_unavailable",
        ),
        ProtocolErrorCode::PermissionDenied => {
            (LocalFuseFilesState::PermissionDenied, "permission_denied")
        }
        _ => (LocalFuseFilesState::Error, "error"),
    }
}

fn local_io_state(error: &std::io::Error) -> (LocalFuseFilesState, &'static str) {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            (LocalFuseFilesState::PermissionDenied, "permission_denied")
        }
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotConnected => {
            (LocalFuseFilesState::FuseOffline, "fuse_offline")
        }
        _ => (LocalFuseFilesState::Error, "local_io"),
    }
}

fn local_io_message(error: &std::io::Error, action: &str) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            format!("Permission was denied while trying to {action}.")
        }
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotConnected => {
            "The Docker filesystem path is no longer available. Retry after checking the mount."
                .into()
        }
        _ => format!("Could not {action}: {error}"),
    }
}

fn daemon_error_message(error: &ClientError) -> String {
    match error {
        ClientError::MissingRuntimeDirectory
        | ClientError::SocketUnavailable(_)
        | ClientError::Disconnected(_)
        | ClientError::Io(_) => "TuxStack service is not running.".into(),
        _ => format!("TuxStack service is unavailable: {error}"),
    }
}

fn breadcrumb_list(controller: &LocalFuseFilesController) -> QVariantList {
    let mut list = QVariantList::default();
    for breadcrumb in controller.breadcrumbs() {
        let mut map = QVariantMap::default();
        map.insert(QString::from("label"), qv(&breadcrumb.label));
        map.insert(QString::from("pathToken"), qv(&breadcrumb.path_token));
        list.append(QVariant::from(&map));
    }
    list
}

fn push_property(list: &mut QVariantList, label: &str, value: &str) {
    let mut map = QVariantMap::default();
    map.insert(QString::from("label"), qv(label));
    map.insert(QString::from("value"), qv(value));
    list.append(QVariant::from(&map));
}

fn local_destination(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("A save destination is required.".into());
    }
    let encoded = if let Some(rest) = value.strip_prefix("file://") {
        if rest.starts_with('/') {
            rest
        } else if let Some(rest) = rest.strip_prefix("localhost/") {
            return decode_destination(&format!("/{rest}"));
        } else {
            return Err("The destination must be a local file URL.".into());
        }
    } else if let Some(rest) = value.strip_prefix("file:") {
        rest
    } else if value.contains("://") {
        return Err("The destination must be a local path.".into());
    } else {
        return Ok(PathBuf::from(value));
    };
    decode_destination(encoded)
}

fn decode_destination(value: &str) -> Result<PathBuf, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("The destination contains an invalid URL escape.".into());
            }
            let high = hex_value(bytes[index + 1])
                .ok_or_else(|| "The destination contains an invalid URL escape.".to_string())?;
            let low = hex_value(bytes[index + 2])
                .ok_or_else(|| "The destination contains an invalid URL escape.".to_string())?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    if output.contains(&0) {
        return Err("The destination is invalid.".into());
    }
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(output)))
}

fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut result = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if *byte == b'/' {
            result.push('/');
        } else if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            result.push(*byte as char);
        } else {
            result.push('%');
            result.push(hex_digit(byte >> 4));
            result.push(hex_digit(byte & 0x0f));
        }
    }
    result
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn cancel(slot: &mut Option<CancellationToken>) {
    if let Some(token) = slot.take() {
        token.cancel();
    }
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&qstring(value))
}

fn qstring(value: &str) -> QString {
    QString::from(value)
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_encode_raw_unix_bytes_without_process_launching() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'/', b't', b'm', b'p', b'/', b'f', 0x80,
        ]));
        assert_eq!(file_url(&path), "file:///tmp/f%80");
    }

    #[test]
    fn save_destination_decodes_local_urls_and_rejects_remote_urls() {
        assert_eq!(
            local_destination("file:///tmp/My%20File.txt").unwrap(),
            PathBuf::from("/tmp/My File.txt")
        );
        assert!(local_destination("file://remote/tmp/file").is_err());
        assert!(local_destination("https://example.invalid/file").is_err());
        assert!(local_destination("file:///tmp/%00").is_err());
    }

    #[test]
    fn descriptor_never_calls_a_snapshot_live() {
        let descriptor = ProviderDescriptor {
            kind: ProviderKind::ContainerRootfsSnapshot,
            consistency: ConsistencyMode::Snapshot {
                captured_at_unix_ms: 1_700_000_000_000,
                generation: 7,
            },
            source: Some("container".into()),
            capabilities: ProviderCapabilities::REFRESH,
            status: ProviderStatus::Ready,
        };
        let presentation = ProviderPresentation::from_descriptor(&descriptor);
        assert_eq!(presentation.consistency, "Snapshot · Read-only");
        assert!(!presentation.consistency.contains("Live"));
        assert_eq!(presentation.refresh_text, "Refresh Snapshot");
    }

    #[test]
    fn qml_fixture_uses_qt_url_opening_and_exposes_no_mutating_actions() {
        let view = include_str!("../../qml/components/LocalFuseFilesView.qml");
        assert!(view.contains("Qt.openUrlExternally(url)"));
        assert!(view.contains("Open in File Manager"));
        assert!(view.contains("Open in Volumes"));
        assert!(view.contains("Open Host Folder"));
        for forbidden in ["xdg-open", "Delete", "Rename", "Upload", "New Folder"] {
            assert!(
                !view.contains(forbidden),
                "forbidden Files action: {forbidden}"
            );
        }
    }
}
