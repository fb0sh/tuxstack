//! Image, network, and volume list models.
//!
//! These are read-mostly list models with a single `refresh` invokable
//! each; Docker I/O runs on the Tokio runtime.

pub use crate::bridge::image_bridge::ImageListModelRust;
pub use crate::bridge::image_file_bridge::ImageFileListModelRust;
pub use crate::bridge::network_bridge::NetworkListModelRust;
pub use crate::bridge::volume_bridge::VolumeListModelRust;
pub use crate::bridge::volume_file_bridge::VolumeFileListModelRust;

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

        include!("cxx-qt-lib/core/qlist/qlist_i32.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
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
        /// Unified Docker volume state, controller, detail object and list model.
        ///
        /// Property names intentionally follow the existing QML contract rather
        /// than CXX-Qt's default snake_case conversion.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY)]
        #[qproperty(QString, sort_mode, cxx_name = "sortMode", READ, NOTIFY)]
        #[qproperty(QString, list_state, cxx_name = "listState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(i32, volume_count, cxx_name = "volumeCount")]
        #[qproperty(i32, in_use_count, cxx_name = "inUseCount")]
        #[qproperty(i32, unused_count, cxx_name = "unusedCount")]
        #[qproperty(i64, known_total_size_bytes, cxx_name = "knownTotalSizeBytes")]
        #[qproperty(QString, known_total_size_text, cxx_name = "knownTotalSizeText")]
        #[qproperty(i32, known_size_count, cxx_name = "knownSizeCount")]
        #[qproperty(i32, unknown_size_count, cxx_name = "unknownSizeCount")]
        #[qproperty(
            bool,
            global_operation_in_progress,
            cxx_name = "globalOperationInProgress"
        )]
        #[qproperty(bool, operation_in_progress, cxx_name = "operationInProgress")]
        #[qproperty(QString, selected_volume_name, cxx_name = "selectedVolumeName")]
        #[qproperty(bool, selected_volume_busy, cxx_name = "selectedVolumeBusy")]
        #[qproperty(QString, detail_state, cxx_name = "detailState")]
        #[qproperty(QString, detail_error_kind, cxx_name = "detailErrorKind")]
        #[qproperty(QString, detail_error, cxx_name = "detailError")]
        #[qproperty(QString, detail_name, cxx_name = "detailName")]
        #[qproperty(QString, detail_driver, cxx_name = "detailDriver")]
        #[qproperty(QString, detail_scope, cxx_name = "detailScope")]
        #[qproperty(QString, detail_mountpoint, cxx_name = "detailMountpoint")]
        #[qproperty(QString, detail_created_text, cxx_name = "detailCreatedText")]
        #[qproperty(i64, detail_size_bytes, cxx_name = "detailSizeBytes")]
        #[qproperty(bool, detail_size_known, cxx_name = "detailSizeKnown")]
        #[qproperty(QString, detail_size_text, cxx_name = "detailSizeText")]
        #[qproperty(QString, detail_ref_count_text, cxx_name = "detailRefCountText")]
        #[qproperty(bool, detail_anonymous, cxx_name = "detailAnonymous")]
        #[qproperty(QVariant, detail)]
        #[qproperty(QList_QVariant, general_model, cxx_name = "generalModel")]
        #[qproperty(QList_QVariant, used_by_model, cxx_name = "usedByModel")]
        #[qproperty(QList_QVariant, label_model, cxx_name = "labelModel")]
        #[qproperty(QList_QVariant, option_model, cxx_name = "optionModel")]
        #[qproperty(QList_QVariant, status_model, cxx_name = "statusModel")]
        #[qproperty(i32, label_count, cxx_name = "labelCount")]
        #[qproperty(i32, option_count, cxx_name = "optionCount")]
        #[qproperty(i32, status_count, cxx_name = "statusCount")]
        #[qproperty(bool, creating)]
        #[qproperty(QString, create_error_message, cxx_name = "createErrorMessage")]
        #[qproperty(bool, remove_preparation_active, cxx_name = "removePreparationActive")]
        #[qproperty(QString, removing_volume_name, cxx_name = "removingVolumeName")]
        #[qproperty(QString, remove_error_message, cxx_name = "removeErrorMessage")]
        #[qproperty(bool, prune_preparation_active, cxx_name = "prunePreparationActive")]
        #[qproperty(bool, pruning)]
        #[qproperty(
            QList_QVariant,
            prune_candidate_model,
            cxx_name = "pruneCandidateModel"
        )]
        #[qproperty(QString, prune_known_size_text, cxx_name = "pruneKnownSizeText")]
        #[qproperty(i32, prune_unknown_size_count, cxx_name = "pruneUnknownSizeCount")]
        #[qproperty(QString, prune_error_message, cxx_name = "pruneErrorMessage")]
        #[qproperty(QString, exporting_volume_name, cxx_name = "exportingVolumeName")]
        #[qproperty(QString, export_status, cxx_name = "exportStatus")]
        #[qproperty(QString, export_error_message, cxx_name = "exportErrorMessage")]
        #[qproperty(QString, cloning_source_name, cxx_name = "cloningSourceName")]
        #[qproperty(QString, clone_status, cxx_name = "cloneStatus")]
        #[qproperty(QString, clone_error_message, cxx_name = "cloneErrorMessage")]
        #[qproperty(bool, zstd_available, cxx_name = "zstdAvailable")]
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

        #[qsignal]
        #[cxx_name = "removePrepared"]
        fn remove_prepared(
            self: Pin<&mut Self>,
            volume_name: QString,
            driver: QString,
            size_text: QString,
            used_by_count: i32,
            mountpoint: QString,
        );

        #[qsignal]
        #[cxx_name = "removePreparationFailed"]
        fn remove_preparation_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "prunePrepared"]
        fn prune_prepared(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "prunePreparationFailed"]
        fn prune_preparation_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "volumeCreated"]
        fn volume_created(self: Pin<&mut Self>, volume_name: QString);

        #[qsignal]
        #[cxx_name = "volumeRemoved"]
        fn volume_removed(self: Pin<&mut Self>, volume_name: QString);

        #[qsignal]
        #[cxx_name = "volumesPruned"]
        fn volumes_pruned(
            self: Pin<&mut Self>,
            removed_count: i32,
            reclaimed_size_text: QString,
            unknown_size_count: i32,
        );

        #[qsignal]
        #[cxx_name = "exportCompleted"]
        fn export_completed(self: Pin<&mut Self>, volume_name: QString, destination_path: QString);

        #[qsignal]
        #[cxx_name = "cloneCompleted"]
        fn clone_completed(self: Pin<&mut Self>, source_volume: QString, target_volume: QString);

        #[qsignal]
        #[cxx_name = "containerNavigationRequested"]
        fn container_navigation_requested(self: Pin<&mut Self>, container_id: QString);

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
        #[cxx_name = "selectVolume"]
        fn select_volume(self: Pin<&mut Self>, volume_name: &QString);

        #[qinvokable]
        #[cxx_name = "reloadSelectedVolume"]
        fn reload_selected_volume(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);

        #[qinvokable]
        #[cxx_name = "createVolume"]
        fn create_volume(
            self: Pin<&mut Self>,
            name: &QString,
            driver: &QString,
            driver_options: &QList_QVariant,
            labels: &QList_QVariant,
        );

        #[qinvokable]
        #[cxx_name = "cancelCreate"]
        fn cancel_create(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "prepareRemoveVolume"]
        fn prepare_remove_volume(self: Pin<&mut Self>, volume_name: &QString);

        #[qinvokable]
        #[cxx_name = "removeVolume"]
        fn remove_volume(self: Pin<&mut Self>, volume_name: &QString, force: bool);

        #[qinvokable]
        #[cxx_name = "preparePruneVolumes"]
        fn prepare_prune_volumes(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "pruneVolumes"]
        fn prune_volumes(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "cancelPrune"]
        fn cancel_prune(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "exportVolume"]
        fn export_volume(
            self: Pin<&mut Self>,
            volume_name: &QString,
            destination: &QString,
            format: &QString,
        );

        #[qinvokable]
        #[cxx_name = "cancelExport"]
        fn cancel_export(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "cloneVolume"]
        fn clone_volume(
            self: Pin<&mut Self>,
            source_volume: &QString,
            target_name: &QString,
            target_driver: &QString,
            target_driver_options: &QList_QVariant,
            copy_labels: bool,
            cleanup_failed: bool,
        );

        #[qinvokable]
        #[cxx_name = "cancelClone"]
        fn cancel_clone(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "navigateToContainer"]
        fn navigate_to_container(self: Pin<&mut Self>, container_id: &QString);

        #[qinvokable]
        #[cxx_name = "setLabelSearchQuery"]
        fn set_label_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[cxx_name = "setLabelSortAscending"]
        fn set_label_sort_ascending(self: Pin<&mut Self>, ascending: bool);

        #[qinvokable]
        #[cxx_name = "setOptionSearchQuery"]
        fn set_option_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[cxx_name = "setOptionSortAscending"]
        fn set_option_sort_ascending(self: Pin<&mut Self>, ascending: bool);

        #[qinvokable]
        #[cxx_name = "setStatusSearchQuery"]
        fn set_status_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[cxx_name = "setStatusSortAscending"]
        fn set_status_sort_ascending(self: Pin<&mut Self>, ascending: bool);

        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for VolumeFileListModel {}

    unsafe extern "RustQt" {
        /// Read-only Docker volume file browser model/controller.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, files_state, cxx_name = "filesState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, volume_name, cxx_name = "volumeName")]
        #[qproperty(QString, current_path, cxx_name = "currentPath")]
        #[qproperty(bool, can_go_back, cxx_name = "canGoBack")]
        #[qproperty(bool, can_go_up, cxx_name = "canGoUp")]
        #[qproperty(bool, show_hidden, cxx_name = "showHidden", READ, NOTIFY)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn")]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending")]
        #[qproperty(bool, directories_first, cxx_name = "directoriesFirst")]
        #[qproperty(QString, selected_entry_path, cxx_name = "selectedEntryPath")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(bool, truncated)]
        #[qproperty(QList_QVariant, breadcrumb_model, cxx_name = "breadcrumbModel")]
        #[qproperty(bool, active, READ, NOTIFY)]
        #[qproperty(bool, preview_loading, cxx_name = "previewLoading")]
        #[qproperty(QString, preview_name, cxx_name = "previewName")]
        #[qproperty(QString, preview_path, cxx_name = "previewPath")]
        #[qproperty(QString, preview_kind, cxx_name = "previewKind")]
        #[qproperty(QString, preview_text, cxx_name = "previewText")]
        #[qproperty(QString, preview_mime, cxx_name = "previewMime")]
        #[qproperty(QString, preview_size_text, cxx_name = "previewSizeText")]
        #[qproperty(bool, preview_truncated, cxx_name = "previewTruncated")]
        #[qproperty(bool, preview_is_image, cxx_name = "previewIsImage")]
        #[qproperty(bool, preview_is_text, cxx_name = "previewIsText")]
        #[qproperty(bool, preview_is_binary, cxx_name = "previewIsBinary")]
        #[qproperty(QString, preview_parse_error, cxx_name = "previewParseError")]
        #[qproperty(QString, preview_image_path, cxx_name = "previewImagePath")]
        #[qproperty(QString, preview_error, cxx_name = "previewError")]
        #[qproperty(bool, download_in_progress, cxx_name = "downloadInProgress")]
        #[qproperty(i64, download_bytes_written, cxx_name = "downloadBytesWritten")]
        #[qproperty(QString, download_progress_text, cxx_name = "downloadProgressText")]
        #[qproperty(QString, download_error, cxx_name = "downloadError")]
        #[qproperty(QList_QVariant, properties_model, cxx_name = "propertiesModel")]
        type VolumeFileListModel = super::VolumeFileListModelRust;

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
        #[cxx_name = "previewReady"]
        fn preview_ready(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "downloadCompleted"]
        fn download_completed(self: Pin<&mut Self>, destination_path: QString);

        #[qsignal]
        #[cxx_name = "downloadFailed"]
        fn download_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "propertiesReady"]
        fn properties_ready(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "symlinkBlocked"]
        fn symlink_blocked(self: Pin<&mut Self>, message: QString);

        #[qinvokable]
        #[rust_name = "update_active"]
        #[cxx_name = "setActive"]
        fn set_active(self: Pin<&mut Self>, active: bool);

        #[qinvokable]
        #[cxx_name = "openVolume"]
        fn open_volume(self: Pin<&mut Self>, volume_name: &QString);

        #[qinvokable]
        #[cxx_name = "closeVolume"]
        fn close_volume(self: Pin<&mut Self>);

        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "openEntry"]
        fn open_entry(self: Pin<&mut Self>, path: &QString);

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
        #[rust_name = "update_search_query"]
        #[cxx_name = "setSearchQuery"]
        fn set_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[rust_name = "update_show_hidden"]
        #[cxx_name = "setShowHidden"]
        fn set_show_hidden(self: Pin<&mut Self>, show: bool);

        #[qinvokable]
        #[rust_name = "update_sort"]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut Self>, column: &QString, descending: bool);

        #[qinvokable]
        #[cxx_name = "toggleSort"]
        fn toggle_sort(self: Pin<&mut Self>, column: &QString);

        #[qinvokable]
        #[cxx_name = "selectEntry"]
        fn select_entry(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        #[cxx_name = "previewEntry"]
        fn preview_entry(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        #[cxx_name = "cancelPreview"]
        fn cancel_preview(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "downloadEntry"]
        fn download_entry(self: Pin<&mut Self>, path: &QString, destination: &QString);

        #[qinvokable]
        #[cxx_name = "cancelDownload"]
        fn cancel_download(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "loadProperties"]
        fn load_properties(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        fn retry(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);

        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for ImageFileListModel {}

    unsafe extern "RustQt" {
        /// Read-only Docker image file browser model/controller.
        ///
        /// Browsing an image's filesystem runs a hardened temporary helper
        /// container created from the image itself; see
        /// `tuxstack-docker-core::services::image_files`.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, files_state, cxx_name = "filesState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, image_id, cxx_name = "imageId")]
        #[qproperty(QString, current_path, cxx_name = "currentPath")]
        #[qproperty(bool, can_go_back, cxx_name = "canGoBack")]
        #[qproperty(bool, can_go_up, cxx_name = "canGoUp")]
        #[qproperty(bool, show_hidden, cxx_name = "showHidden", READ, NOTIFY)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn")]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending")]
        #[qproperty(bool, directories_first, cxx_name = "directoriesFirst")]
        #[qproperty(QString, selected_entry_path, cxx_name = "selectedEntryPath")]
        #[qproperty(bool, loading)]
        #[qproperty(i32, count)]
        #[qproperty(bool, truncated)]
        #[qproperty(QList_QVariant, breadcrumb_model, cxx_name = "breadcrumbModel")]
        #[qproperty(bool, active, READ, NOTIFY)]
        #[qproperty(bool, preview_loading, cxx_name = "previewLoading")]
        #[qproperty(QString, preview_name, cxx_name = "previewName")]
        #[qproperty(QString, preview_path, cxx_name = "previewPath")]
        #[qproperty(QString, preview_kind, cxx_name = "previewKind")]
        #[qproperty(QString, preview_text, cxx_name = "previewText")]
        #[qproperty(QString, preview_mime, cxx_name = "previewMime")]
        #[qproperty(QString, preview_size_text, cxx_name = "previewSizeText")]
        #[qproperty(bool, preview_truncated, cxx_name = "previewTruncated")]
        #[qproperty(bool, preview_is_image, cxx_name = "previewIsImage")]
        #[qproperty(bool, preview_is_text, cxx_name = "previewIsText")]
        #[qproperty(bool, preview_is_binary, cxx_name = "previewIsBinary")]
        #[qproperty(QString, preview_parse_error, cxx_name = "previewParseError")]
        #[qproperty(QString, preview_image_path, cxx_name = "previewImagePath")]
        #[qproperty(QString, preview_error, cxx_name = "previewError")]
        #[qproperty(bool, download_in_progress, cxx_name = "downloadInProgress")]
        #[qproperty(i64, download_bytes_written, cxx_name = "downloadBytesWritten")]
        #[qproperty(QString, download_progress_text, cxx_name = "downloadProgressText")]
        #[qproperty(QString, download_error, cxx_name = "downloadError")]
        #[qproperty(QList_QVariant, properties_model, cxx_name = "propertiesModel")]
        type ImageFileListModel = super::ImageFileListModelRust;

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
        #[cxx_name = "previewReady"]
        fn preview_ready(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "downloadCompleted"]
        fn download_completed(self: Pin<&mut Self>, destination_path: QString);

        #[qsignal]
        #[cxx_name = "downloadFailed"]
        fn download_failed(self: Pin<&mut Self>, message: QString);

        #[qsignal]
        #[cxx_name = "propertiesReady"]
        fn properties_ready(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "symlinkBlocked"]
        fn symlink_blocked(self: Pin<&mut Self>, message: QString);

        #[qinvokable]
        #[rust_name = "update_active"]
        #[cxx_name = "setActive"]
        fn set_active(self: Pin<&mut Self>, active: bool);

        #[qinvokable]
        #[cxx_name = "openImage"]
        fn open_image(self: Pin<&mut Self>, image_id: &QString);

        #[qinvokable]
        #[cxx_name = "closeImage"]
        fn close_image(self: Pin<&mut Self>);

        #[qinvokable]
        fn refresh(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "openEntry"]
        fn open_entry(self: Pin<&mut Self>, path: &QString);

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
        #[rust_name = "update_search_query"]
        #[cxx_name = "setSearchQuery"]
        fn set_search_query(self: Pin<&mut Self>, query: &QString);

        #[qinvokable]
        #[rust_name = "update_show_hidden"]
        #[cxx_name = "setShowHidden"]
        fn set_show_hidden(self: Pin<&mut Self>, show: bool);

        #[qinvokable]
        #[rust_name = "update_sort"]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut Self>, column: &QString, descending: bool);

        #[qinvokable]
        #[cxx_name = "toggleSort"]
        fn toggle_sort(self: Pin<&mut Self>, column: &QString);

        #[qinvokable]
        #[cxx_name = "selectEntry"]
        fn select_entry(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        #[cxx_name = "previewEntry"]
        fn preview_entry(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        #[cxx_name = "cancelPreview"]
        fn cancel_preview(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "downloadEntry"]
        fn download_entry(self: Pin<&mut Self>, path: &QString, destination: &QString);

        #[qinvokable]
        #[cxx_name = "cancelDownload"]
        fn cancel_download(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "loadProperties"]
        fn load_properties(self: Pin<&mut Self>, path: &QString);

        #[qinvokable]
        fn retry(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "loadMore"]
        fn load_more(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setConnectionState"]
        fn set_connection_state(self: Pin<&mut Self>, docker_status: i32, message: &QString);

        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }
}
