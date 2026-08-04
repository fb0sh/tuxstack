//! Image, network, and volume list models.
//!
//! These are read-mostly list models with a single `refresh` invokable
//! each; Docker I/O runs on the Tokio runtime.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};

use crate::app_state::{VolumeRow, get_services, map_docker_error};
pub use crate::bridge::image_bridge::ImageListModelRust;
pub use crate::bridge::network_bridge::NetworkListModelRust;

/// Build a QVariant from a string (String → QString → QVariant).
fn qv(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(< QAbstractListModel >);
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
    }

    // Image role IDs are implemented as constants 257..=272 in
    // image_bridge.rs because CXX-Qt 0.9 QEnums cannot have the explicit
    // Qt::UserRole + 1 discriminant required by QAbstractItemModel.

    impl cxx_qt::Threading for ImageListModel {}

    unsafe extern "RustQt" {
        /// Unified Docker image state, controller, detail object and list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_query, cxx_name = "currentSearchQuery")]
        #[qproperty(QString, sort_mode, cxx_name = "currentSortMode")]
        #[qproperty(i32, status)]
        #[qproperty(QString, state_name, cxx_name = "state")]
        #[qproperty(QString, status_text, cxx_name = "statusText")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(i32, total_image_count, cxx_name = "totalImageCount")]
        #[qproperty(i32, in_use_count, cxx_name = "inUseCount")]
        #[qproperty(i32, unused_count, cxx_name = "unusedCount")]
        #[qproperty(i64, total_size_bytes, cxx_name = "totalSizeBytes")]
        #[qproperty(QString, total_size_text, cxx_name = "totalSizeText")]
        #[qproperty(QString, selected_image_id, cxx_name = "selectedImageId")]
        #[qproperty(bool, detail_loading, cxx_name = "detailLoading")]
        #[qproperty(QString, detail_state, cxx_name = "detailState")]
        #[qproperty(QString, detail_error, cxx_name = "detailError")]
        #[qproperty(QString, detail_error_kind, cxx_name = "detailErrorKind")]
        #[qproperty(bool, operation_in_progress, cxx_name = "operationInProgress")]
        #[qproperty(QString, removing_image_id, cxx_name = "removingImageId")]
        #[qproperty(QString, remove_error_message, cxx_name = "removeErrorMessage")]
        #[qproperty(bool, pull_active, cxx_name = "pullActive")]
        #[qproperty(bool, pulling)]
        #[qproperty(QString, pull_status, cxx_name = "pullStatus")]
        #[qproperty(QString, pull_error_message, cxx_name = "pullErrorMessage")]
        #[qproperty(bool, pull_progress_known, cxx_name = "pullProgressKnown")]
        #[qproperty(QString, pull_progress_text, cxx_name = "pullProgressText")]
        #[qproperty(QString, pull_layer_id, cxx_name = "pullLayerId")]
        #[qproperty(i64, pull_current, cxx_name = "pullCurrent")]
        #[qproperty(i64, pull_total, cxx_name = "pullTotal")]
        #[qproperty(f64, pull_percent, cxx_name = "pullPercent")]
        #[qproperty(bool, export_active, cxx_name = "exportActive")]
        #[qproperty(bool, exporting)]
        #[qproperty(i64, export_bytes_written, cxx_name = "exportBytesWritten")]
        #[qproperty(QString, export_bytes_text, cxx_name = "exportBytesText")]
        #[qproperty(QString, export_destination, cxx_name = "exportDestination")]
        #[qproperty(QString, export_status, cxx_name = "exportStatus")]
        #[qproperty(QString, export_error_message, cxx_name = "exportErrorMessage")]
        #[qproperty(QString, detail_id, cxx_name = "detailId")]
        #[qproperty(QString, detail_short_id, cxx_name = "detailShortId")]
        #[qproperty(QString, detail_display_name, cxx_name = "detailDisplayName")]
        #[qproperty(QString, detail_tags, cxx_name = "detailTags")]
        #[qproperty(QString, detail_digests, cxx_name = "detailDigests")]
        #[qproperty(QString, detail_created, cxx_name = "detailCreated")]
        #[qproperty(QString, detail_size, cxx_name = "detailSize")]
        #[qproperty(QString, detail_virtual_size, cxx_name = "detailVirtualSize")]
        #[qproperty(QString, detail_platform, cxx_name = "detailPlatform")]
        #[qproperty(QString, detail_architecture, cxx_name = "detailArchitecture")]
        #[qproperty(QString, detail_os, cxx_name = "detailOs")]
        #[qproperty(QString, detail_author, cxx_name = "detailAuthor")]
        #[qproperty(QString, detail_docker_version, cxx_name = "detailDockerVersion")]
        #[qproperty(QString, detail_comment, cxx_name = "detailComment")]
        #[qproperty(QString, detail_command, cxx_name = "detailCommand")]
        #[qproperty(QString, detail_entrypoint, cxx_name = "detailEntrypoint")]
        #[qproperty(QString, detail_working_dir, cxx_name = "detailWorkingDir")]
        #[qproperty(QString, detail_user, cxx_name = "detailUser")]
        #[qproperty(QString, detail_stop_signal, cxx_name = "detailStopSignal")]
        #[qproperty(QString, detail_shell, cxx_name = "detailShell")]
        #[qproperty(QVariant, detail)]
        #[qproperty(QList_QVariant, environment_rows, cxx_name = "environmentRows")]
        #[qproperty(QList_QVariant, label_rows, cxx_name = "labelRows")]
        #[qproperty(QList_QVariant, usage_rows, cxx_name = "usageRows")]
        #[qproperty(QList_QVariant, environment_model, cxx_name = "environmentModel")]
        #[qproperty(QList_QVariant, label_model, cxx_name = "labelModel")]
        #[qproperty(QList_QVariant, usage_model, cxx_name = "usageModel")]
        type ImageListModel = super::ImageListModelRust;

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
        #[cxx_name = "operationFinished"]
        fn operation_finished(
            self: Pin<&mut Self>,
            operation: QString,
            success: bool,
            message: QString,
        );

        #[qsignal]
        #[cxx_name = "containerNavigationRequested"]
        fn container_navigation_requested(self: Pin<&mut Self>, container_id: QString);

        #[qsignal]
        #[cxx_name = "imageRemoved"]
        fn image_removed(self: Pin<&mut Self>, display_name: QString);

        #[qsignal]
        #[cxx_name = "pullCompleted"]
        fn pull_completed(self: Pin<&mut Self>, image_reference: QString);

        #[qsignal]
        #[cxx_name = "exportCompleted"]
        fn export_completed(self: Pin<&mut Self>, destination_path: QString);

        #[qinvokable]
        fn initialize(self: Pin<&mut Self>);

        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);

        #[qinvokable]
        #[rust_name = "update_search_query"]
        #[cxx_name = "setSearchQuery"]
        fn set_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[rust_name = "update_sort_mode"]
        #[cxx_name = "setSortMode"]
        fn set_sort_mode(self: Pin<&mut Self>, mode: &QString);

        #[qinvokable]
        #[cxx_name = "selectImage"]
        fn select_image(self: Pin<&mut Self>, image_id: &QString);

        #[qinvokable]
        #[cxx_name = "reloadSelectedImage"]
        fn reload_selected_image(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);

        #[qinvokable]
        #[cxx_name = "removeImage"]
        fn remove_image(
            self: Pin<&mut Self>,
            image_id: &QString,
            force: bool,
            prune_children: bool,
        );

        #[qinvokable]
        #[cxx_name = "pullImage"]
        fn pull_image(
            self: Pin<&mut Self>,
            reference: &QString,
            platform: &QString,
            username: &QString,
            password: &QString,
            registry: &QString,
        );

        #[qinvokable]
        #[cxx_name = "cancelPull"]
        fn cancel_pull(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "exportImage"]
        fn export_image(self: Pin<&mut Self>, image_id: &QString, destination: &QString);

        #[qinvokable]
        #[cxx_name = "cancelExport"]
        fn cancel_export(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "requestContainerNavigation"]
        fn open_container(self: Pin<&mut Self>, container_id: &QString);

        #[qinvokable]
        #[cxx_name = "setEnvironmentSearchQuery"]
        fn set_environment_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[cxx_name = "setEnvironmentSortAscending"]
        fn set_environment_sort_ascending(self: Pin<&mut Self>, ascending: bool);

        #[qinvokable]
        #[cxx_name = "setLabelSearchQuery"]
        fn set_label_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[cxx_name = "setLabelSortAscending"]
        fn set_label_sort_ascending(self: Pin<&mut Self>, ascending: bool);

        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for NetworkListModel {}

    unsafe extern "RustQt" {
        /// Unified Docker network state, controller, detail and list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_query, cxx_name = "currentSearchQuery")]
        #[qproperty(QString, sort_mode, cxx_name = "currentSortMode")]
        #[qproperty(i32, status)]
        #[qproperty(QString, state_name, cxx_name = "state")]
        #[qproperty(QString, status_text, cxx_name = "statusText")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(i32, total_network_count, cxx_name = "totalNetworkCount")]
        #[qproperty(QString, selected_network_id, cxx_name = "selectedNetworkId")]
        #[qproperty(bool, detail_loading, cxx_name = "detailLoading")]
        #[qproperty(QString, detail_state, cxx_name = "detailState")]
        #[qproperty(QString, detail_error, cxx_name = "detailError")]
        #[qproperty(QString, detail_error_kind, cxx_name = "detailErrorKind")]
        #[qproperty(QVariant, detail)]
        #[qproperty(QList_QVariant, option_rows, cxx_name = "optionRows")]
        #[qproperty(QList_QVariant, label_rows, cxx_name = "labelRows")]
        #[qproperty(QList_QVariant, subnet_rows, cxx_name = "subnetRows")]
        #[qproperty(QList_QVariant, container_rows, cxx_name = "containerRows")]
        #[qproperty(bool, operation_in_progress, cxx_name = "operationInProgress")]
        #[qproperty(bool, creating)]
        #[qproperty(QString, create_error_message, cxx_name = "createErrorMessage")]
        #[qproperty(bool, remove_preparation_active, cxx_name = "removePreparationActive")]
        #[qproperty(QString, removing_network_id, cxx_name = "removingNetworkId")]
        #[qproperty(QString, remove_error_message, cxx_name = "removeErrorMessage")]
        type NetworkListModel = super::NetworkListModelRust;

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
        #[cxx_name = "operationFinished"]
        fn operation_finished(
            self: Pin<&mut Self>,
            operation: QString,
            success: bool,
            message: QString,
        );

        #[qsignal]
        #[cxx_name = "removePrepared"]
        fn remove_prepared(
            self: Pin<&mut Self>,
            network_id: QString,
            name: QString,
            short_id: QString,
            connected_container_count: i32,
        );

        #[qsignal]
        #[cxx_name = "removePreparationFailed"]
        fn remove_preparation_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "networkCreated"]
        fn network_created(self: Pin<&mut Self>, name: QString);

        #[qsignal]
        #[cxx_name = "networkRemoved"]
        fn network_removed(self: Pin<&mut Self>, name: QString);

        #[qinvokable]
        fn initialize(self: Pin<&mut Self>);

        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);

        #[qinvokable]
        #[rust_name = "update_search_query"]
        #[cxx_name = "setSearchQuery"]
        fn set_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[rust_name = "update_sort_mode"]
        #[cxx_name = "setSortMode"]
        fn set_sort_mode(self: Pin<&mut Self>, mode: &QString);

        #[qinvokable]
        #[cxx_name = "selectNetwork"]
        fn select_network(self: Pin<&mut Self>, network_id: &QString);

        #[qinvokable]
        #[cxx_name = "reloadSelectedNetwork"]
        fn reload_selected_network(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);

        #[qinvokable]
        #[cxx_name = "createNetwork"]
        fn create_network(
            self: Pin<&mut Self>,
            name: &QString,
            driver: &QString,
            subnet: &QString,
            gateway: &QString,
            flags: i32,
            labels_text: &QString,
        );

        #[qinvokable]
        #[cxx_name = "prepareRemoveNetwork"]
        fn prepare_remove_network(self: Pin<&mut Self>, network_id: &QString);

        #[qinvokable]
        #[cxx_name = "removeNetwork"]
        fn remove_network(self: Pin<&mut Self>, network_id: &QString);

        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for VolumeListModel {}

    unsafe extern "RustQt" {
        /// Volume list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        #[qproperty(i32, status)]
        #[qproperty(QString, status_text)]
        type VolumeListModel = super::VolumeListModelRust;

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

        /// Reload the volume list.
        #[qinvokable]
        #[rust_name = "refresh"]
        fn refresh(self: Pin<&mut Self>);
    }
}

/// Rust state for [`qobject::VolumeListModel`].
#[derive(Default)]
pub struct VolumeListModelRust {
    pub(crate) rows: Vec<VolumeRow>,
    search_text: QString,
    status: i32,
    status_text: QString,
}

impl qobject::VolumeListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            0 => qv(&row.name),
            1 => qv(&row.driver),
            2 => qv(&row.mountpoint),
            3 => qv(&row.scope),
            4 => qv(&row.created_at),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(0, "name".into());
        hash.insert(1, "driver".into());
        hash.insert(2, "mountpoint".into());
        hash.insert(3, "scope".into());
        hash.insert(4, "createdAt".into());
        hash
    }

    /// Reload the volume list.
    pub fn refresh(mut self: Pin<&mut Self>) {
        self.as_mut().refresh_kind();
    }
}

impl qobject::VolumeListModel {
    fn refresh_kind(mut self: Pin<&mut Self>) {
        let Some(services) = get_services() else {
            self.as_mut().set_status(5);
            self.as_mut()
                .set_status_text(QString::from("Not connected to Docker Engine."));
            return;
        };
        self.as_mut().set_status(1); // loading
        let search = self.search_text().to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let options = tuxstack_docker_core::services::volumes::ListVolumesOptions {
                search: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
            };
            let result = services.volumes.list_volumes(&options).await;
            qt_thread
                .queue(move |mut model| match result {
                    Ok(volumes) => {
                        let rows: Vec<VolumeRow> = volumes
                            .into_iter()
                            .map(|v| VolumeRow {
                                name: v.name,
                                driver: v.driver,
                                mountpoint: v.mountpoint,
                                scope: v.scope,
                                created_at: v
                                    .created_at
                                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default(),
                            })
                            .collect();
                        model.as_mut().apply_volumes(rows);
                    }
                    Err(e) => {
                        model.as_mut().set_status(4);
                        model
                            .as_mut()
                            .set_status_text(QString::from(map_docker_error(&e).user_message()));
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "Qt object destroyed before async result delivery"));
        });
    }

    fn apply_volumes(mut self: Pin<&mut Self>, rows: Vec<VolumeRow>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        let status = if self.rows.is_empty() { 3 } else { 2 };
        self.as_mut().set_status(status);
    }
}
