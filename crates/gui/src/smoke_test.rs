//! GUI smoke tests (in-process, headless).
//!
//! These instantiate every exported QML component with an offscreen QPA
//! platform, then load the complete UI. This catches failures in pages that
//! Main.qml declares lazily and does not instantiate on startup.

#![cfg(test)]

use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QByteArray, QGuiApplication, QQmlApplicationEngine, QQmlEngine, QUrl};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Serializes tests that create Qt GUI objects (QGuiApplication, QML
/// engines). Creating a second `QGuiApplication` from another thread while
/// one already exists is undefined behavior in Qt and reliably deadlocks
/// the test process, so engine-backed smoke tests must never overlap.
static QT_GUI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn assert_qml_loads(url: &str) {
    assert_qml_source_loads(url, None);
}

fn assert_qml_source_loads(url: &str, source: Option<&str>) {
    let mut engine = QQmlApplicationEngine::new();
    let created = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicBool::new(false));

    if let Some(mut engine) = engine.as_mut() {
        {
            let qml_engine: Pin<&mut QQmlEngine> = engine.as_mut().upcast_pin();
            qml_engine.set_output_warnings_to_standard_error(true);
        }

        let count = created.clone();
        engine
            .as_mut()
            .on_object_created(move |_, object, _| {
                if !object.is_null() {
                    count.fetch_add(1, Ordering::SeqCst);
                }
            })
            .release();

        let load_failed = failed.clone();
        engine
            .on_object_creation_failed(move |_, failed_url| {
                load_failed.store(true, Ordering::SeqCst);
                eprintln!("QML root object creation failed: {failed_url}");
            })
            .release();
    }

    if let Some(engine) = engine.as_mut() {
        let url = QUrl::from(url);
        if let Some(source) = source {
            engine.load_data(&QByteArray::from(source), &url);
        } else {
            engine.load(&url);
        }
    }

    assert!(
        !failed.load(Ordering::SeqCst),
        "{url} reported an object creation failure"
    );
    assert!(
        created.load(Ordering::SeqCst) > 0,
        "{url} must produce a non-null root object"
    );
}

#[test]
fn containers_page_keeps_a_permanent_blankable_detail_panel() {
    let source = include_str!("../qml/pages/ContainersPage.qml");
    let detail = include_str!("../qml/components/containers/ContainerDetailPanel.qml");
    assert!(source.contains("RowLayout {"));
    assert!(source.contains("ContainerListPanel {"));
    assert!(source.contains("ContainerDetailPanel {"));
    assert!(!source.contains("Loader {"));
    assert!(source.contains("containersModel.initialize()"));
    assert!(source.contains("pendingContainerId"));
    assert!(source.contains("CreateContainerDialog"));
    assert!(source.contains("onCreateRequested"));
    assert!(detail.contains("selectionKind === \"container\""));
    assert!(detail.contains("selectionKind === \"group\""));
    assert!(detail.contains("selectionKind !== \"none\""));
    assert!(detail.contains("ContainerStatsView"));
    assert!(detail.contains("ContainerLogsView"));
    assert!(detail.contains("ContainerTerminalView"));
    assert!(detail.contains("ContainerFilesView"));
    assert!(!detail.contains("No container selected"));
    assert!(!detail.contains("Select a container"));
}

#[test]
fn main_wires_the_long_lived_containers_model_and_events() {
    let source = include_str!("../qml/Main.qml");
    assert!(source.contains("ContainersListModel {"));
    assert!(source.contains("id: containersModel"));
    assert!(source.contains("containersModel.shutdown()"));
    assert!(source.contains("ContainerTerminalModel {"));
    assert!(source.contains("containerTerminalModel.shutdown()"));
    assert!(source.contains("containersModel.setConnectionState(appController.dockerStatus,"));
    assert!(source.contains("refreshThrottled(containersModel, \"containers\")"));
    assert!(source.contains("containersModel: containersModel"));
}

#[test]
fn images_page_keeps_a_permanent_detail_panel() {
    let source = include_str!("../qml/pages/ImagesPage.qml");
    assert!(source.contains("RowLayout {"));
    assert!(source.contains("ImageListPanel {"));
    assert!(source.contains("ImageDetailPanel {"));
    assert!(
        !source.contains("Loader {"),
        "the detail layout node must never be conditionally unloaded"
    );
    assert!(source.contains("Component.onCompleted"));
    assert!(source.contains("imagesModel.initialize()"));
}

#[test]
fn main_wires_the_long_lived_volume_model() {
    let source = include_str!("../qml/Main.qml");
    assert!(source.contains("VolumeListModel {"));
    assert!(source.contains("id: volumesModel"));
    assert!(source.contains("VolumeFileListModel {"));
    assert!(source.contains("id: volumeFilesModel"));
    assert!(source.contains("volumesModel.shutdown()"));
    assert!(source.contains("volumeFilesModel.shutdown()"));
    assert!(source.contains("volumesModel.setConnectionState(appController.dockerStatus,"));
    assert!(
        source.contains("volumeFilesModel.setConnectionState(appController.dockerStatus,"),
        "Files model must receive connection-state replay like the other models"
    );
    assert!(
        source.contains("function onDockerChanged(kind)"),
        "Main.qml must handle event-driven dockerChanged notifications"
    );
    assert!(source.contains("refreshThrottled(imagesModel, \"images\")"));
    assert!(source.contains("refreshThrottled(networksModel, \"networks\")"));
    // Containers and Volumes kinds both refresh volumesModel; the throttle
    // key keeps a single burst from rebuilding the volume list twice.
    assert_eq!(
        source
            .matches("refreshThrottled(volumesModel, \"volumes\")")
            .count(),
        2,
        "containers and volumes kinds must both map to volumesModel"
    );
    assert!(source.contains("appController.refreshOverview()"));
    assert!(source.contains("VolumesPage {"));
    assert!(source.contains("volumesModel: volumesModel"));
    assert!(source.contains("filesModel: volumeFilesModel"));
    assert!(source.contains("onInitializationRequested:"));
    assert!(source.contains("onRetryConnectionRequested: appController.startup()"));
    assert!(source.contains("root.showPassiveNotification(message)"));
    assert!(source.contains("target: volumesModel"));
    assert!(source.contains("root.navigateToContainer(containerId)"));
}

#[test]
fn volumes_page_keeps_a_permanent_detail_panel() {
    let source = include_str!("../qml/pages/VolumesPage.qml");
    assert!(source.contains("RowLayout {"));
    assert!(source.contains("VolumeListPanel {"));
    assert!(source.contains("VolumeDetailPanel {"));
    assert!(
        !source.contains("Loader {"),
        "the volume detail layout node must never be conditionally unloaded"
    );
    assert!(source.contains("Component.onCompleted"));
    assert!(source.contains("root.volumesModel.initialize()"));
}

#[test]
fn volume_files_view_uses_single_fill_height_content_area() {
    let source = include_str!("../qml/components/VolumeFilesView.qml");
    assert!(source.contains("id: fileArea"));
    assert!(source.contains("Layout.fillHeight: true"));
    assert!(source.contains("anchors.centerIn: parent"));
    // Overlays must not be ColumnLayout fillHeight siblings of the toolbar.
    assert!(
        source.contains("// Single fill-height content area") || source.contains("id: fileArea"),
        "file table area must own the remaining height"
    );
    let detail = include_str!("../qml/components/VolumeDetailPanel.qml");
    assert!(detail.contains("QQC2.TabBar"));
    assert!(detail.contains("StackLayout"));
    assert!(detail.contains("VolumeFilesView"));
    assert!(detail.contains("VolumeInfoView"));
    assert!(detail.contains("openVolume"));
    assert!(!detail.contains("anchors.centerIn: parent"));
}

#[test]
fn image_detail_panel_has_info_and_files_tabs() {
    let source = include_str!("../qml/components/ImageDetailPanel.qml");
    assert!(source.contains("QQC2.TabBar"));
    assert!(source.contains("StackLayout"));
    assert!(source.contains("ImageFilesView"));
    assert!(
        source.contains("import org.tuxstack.app"),
        "ImageDetailPanel uses I18n.i18nd, so it must import the module that provides the I18n singleton; otherwise the tab texts silently evaluate empty"
    );
    assert!(source.contains("I18n.i18nd(\"tuxstack\", \"Info\")"));
    assert!(source.contains("I18n.i18nd(\"tuxstack\", \"Files\")"));
    assert!(source.contains("openImage"));
    assert!(source.contains("closeImage"));
    assert!(source.contains("setActive"));
    assert!(source.contains("filesTabActiveChanged"));
    let images_page = include_str!("../qml/pages/ImagesPage.qml");
    assert!(images_page.contains("filesModel: root.filesModel"));
    let main = include_str!("../qml/Main.qml");
    assert!(main.contains("ImageFileListModel {"));
    assert!(main.contains("id: imageFilesModel"));
    assert!(main.contains("imageFilesModel.shutdown()"));
    assert!(main.contains("filesModel: imageFilesModel"));
    assert!(main.contains("imageFilesModel.setConnectionState(appController.dockerStatus,"));
}

#[test]
fn networks_page_keeps_a_permanent_detail_panel() {
    let source = include_str!("../qml/pages/NetworksPage.qml");
    assert!(source.contains("RowLayout {"));
    assert!(source.contains("NetworkListPanel {"));
    assert!(source.contains("NetworkDetailPanel {"));
    assert!(
        !source.contains("Loader {"),
        "the network detail layout node must never be conditionally unloaded"
    );
    assert!(source.contains("Component.onCompleted"));
    assert!(source.contains("networksModel.initialize()"));
}

#[test]
fn all_qml_components_load_without_errors() {
    let _qt_guard = QT_GUI_LOCK.lock().unwrap();
    unsafe {
        std::env::set_var("QT_QPA_PLATFORM", "offscreen");
    }
    crate::runtime::init();

    let app = QGuiApplication::new();
    assert!(!app.is_null(), "QGuiApplication must be creatable");

    let base = "qrc:/qt/qml/org/tuxstack/app/qml";
    let components = [
        "components/AppSidebar.qml",
        "components/SidebarSection.qml",
        "components/SidebarItem.qml",
        "components/SidebarCollapseButton.qml",
        "components/PageHeader.qml",
        "components/LoadingView.qml",
        "components/EmptyState.qml",
        "components/ErrorBanner.qml",
        "components/StatusBadge.qml",
        "components/containers/ContainerContextMenu.qml",
        "components/containers/ContainerDetailPanel.qml",
        "components/containers/ContainerFilesView.qml",
        "components/containers/ContainerGroupInfoView.qml",
        "components/containers/ContainerGroupItem.qml",
        "components/containers/ContainerInfoView.qml",
        "components/containers/ContainerListItem.qml",
        "components/containers/ContainerListPanel.qml",
        "components/containers/ContainerLogsView.qml",
        "components/containers/ContainerStatsView.qml",
        "components/containers/ContainerTerminalView.qml",
        "components/ResourceSummaryCard.qml",
        "components/SearchField.qml",
        "components/ImageListPanel.qml",
        "components/ImageListItem.qml",
        "components/ImageDetailPanel.qml",
        "components/PropertySection.qml",
        "components/PropertyList.qml",
        "components/PropertyRow.qml",
        "components/KeyValueTable.qml",
        "components/ImageUsedByList.qml",
        "components/NetworkListPanel.qml",
        "components/NetworkListItem.qml",
        "components/NetworkDetailPanel.qml",
        "components/NetworkContainerList.qml",
        "components/VolumeListPanel.qml",
        "components/VolumeListItem.qml",
        "components/VolumeDetailPanel.qml",
        "components/VolumeInfoView.qml",
        "components/VolumeFilesView.qml",
        "components/ImageFilesView.qml",
        "components/VolumeUsedByList.qml",
        "components/VolumeKeyValueEditor.qml",
        "dialogs/CreateNetworkDialog.qml",
        "dialogs/RemoveNetworkDialog.qml",
        "dialogs/CreateVolumeDialog.qml",
        "dialogs/RemoveVolumeDialog.qml",
        "dialogs/PruneVolumesDialog.qml",
        "dialogs/ExportVolumeDialog.qml",
        "dialogs/CloneVolumeDialog.qml",
        "dialogs/VolumeFilePreviewDialog.qml",
        "dialogs/VolumeFilePropertiesDialog.qml",
        "dialogs/PullImageDialog.qml",
        "dialogs/RemoveImageDialog.qml",
        "dialogs/ExportImageDialog.qml",
        "dialogs/containers/ContainerFilePreviewDialog.qml",
        "dialogs/containers/ContainerFilePropertiesDialog.qml",
        "dialogs/containers/ContainerFileSaveDialog.qml",
        "dialogs/containers/CreateContainerDialog.qml",
        "dialogs/containers/KillContainerDialog.qml",
        "dialogs/containers/RemoveContainerDialog.qml",
        "dialogs/containers/RemoveContainerGroupDialog.qml",
        "dialogs/containers/RenameContainerDialog.qml",
        "dialogs/ErrorDetailsDialog.qml",
        "pages/OverviewPage.qml",
        "pages/ContainersPage.qml",
        "pages/ImagesPage.qml",
        "pages/NetworksPage.qml",
        "pages/VolumesPage.qml",
        "pages/ActivityMonitorPage.qml",
        "pages/CommandsPage.qml",
        "pages/DevicesPage.qml",
        "pages/ComposePage.qml",
        "pages/SettingsPage.qml",
    ];
    for component in components {
        assert_qml_loads(&format!("{base}/{component}"));
    }

    // QAbstractListModel-derived types are not visual roots, so instantiate
    // every registered Rust type beneath an Item.
    let registered_types = [
        "AppController",
        "ContainersListModel",
        "ContainerStatsModel",
        "ContainerLogsModel",
        "ContainerTerminalModel",
        "ContainerFileListModel",
        "ImageListModel",
        "ImageFileListModel",
        "NetworkListModel",
        "VolumeListModel",
        "VolumeFileListModel",
    ];
    for qml_type in registered_types {
        let source = format!("import QtQuick\nimport org.tuxstack.app\nItem {{ {qml_type} {{}} }}");
        assert_qml_source_loads(
            &format!("qrc:/qt/qml/org/tuxstack/app/tests/{qml_type}.qml"),
            Some(&source),
        );
    }

    // Bind the real CXX-Qt object through its public camelCase QML API. This
    // catches accidental snake_case exports that otherwise evaluate undefined.
    let image_model_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    ImageListModel { id: imageModel }
    property int total: imageModel.totalImageCount
    property string size: imageModel.totalSizeText
    property string selected: imageModel.selectedImageId
    property string listState: imageModel.state
    property string detailState: imageModel.detailState
    property var environment: imageModel.environmentModel
    property var labels: imageModel.labelModel
    property var usage: imageModel.usageModel
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/ImageModelApi.qml",
        Some(image_model_api),
    );

    let image_file_model_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    ImageFileListModel { id: filesModel }
    property string state: filesModel.filesState
    property string imageId: filesModel.imageId
    property string currentPath: filesModel.currentPath
    property bool canGoBack: filesModel.canGoBack
    property bool canGoUp: filesModel.canGoUp
    property bool showHidden: filesModel.showHidden
    property string search: filesModel.searchQuery
    property string sort: filesModel.sortColumn
    property bool sortDesc: filesModel.sortDescending
    property string selected: filesModel.selectedEntryPath
    property int count: filesModel.count
    property bool truncated: filesModel.truncated
    property var crumbs: filesModel.breadcrumbModel
    property bool active: filesModel.active
    property string previewError: filesModel.previewError
    property bool downloading: filesModel.downloadInProgress
    property var properties: filesModel.propertiesModel
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/ImageFileModelApi.qml",
        Some(image_file_model_api),
    );

    let network_model_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    NetworkListModel { id: networkModel }
    property string query: networkModel.currentSearchQuery
    property string sort: networkModel.currentSortMode
    property int total: networkModel.totalNetworkCount
    property string selected: networkModel.selectedNetworkId
    property string listState: networkModel.state
    property string detailState: networkModel.detailState
    property string detailError: networkModel.detailError
    property var options: networkModel.optionRows
    property var labels: networkModel.labelRows
    property var subnets: networkModel.subnetRows
    property var containers: networkModel.containerRows
    property bool creating: networkModel.creating
    property string removing: networkModel.removingNetworkId
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/NetworkModelApi.qml",
        Some(network_model_api),
    );

    let volume_model_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    VolumeListModel { id: volumeModel }
    property string query: volumeModel.searchQuery
    property string sort: volumeModel.sortMode
    property string listState: volumeModel.listState
    property string errorKind: volumeModel.errorKind
    property string error: volumeModel.errorMessage
    property bool loading: volumeModel.loading
    property int visibleCount: volumeModel.count
    property int total: volumeModel.volumeCount
    property int inUse: volumeModel.inUseCount
    property int unused: volumeModel.unusedCount
    property double totalBytes: volumeModel.knownTotalSizeBytes
    property string totalSize: volumeModel.knownTotalSizeText
    property int knownSizes: volumeModel.knownSizeCount
    property int unknownSizes: volumeModel.unknownSizeCount
    property bool globallyBusy: volumeModel.globalOperationInProgress
    property bool anyBusy: volumeModel.operationInProgress
    property string selected: volumeModel.selectedVolumeName
    property bool selectedBusy: volumeModel.selectedVolumeBusy
    property string detailState: volumeModel.detailState
    property string detailErrorKind: volumeModel.detailErrorKind
    property string detailError: volumeModel.detailError
    property string detailName: volumeModel.detailName
    property string detailDriver: volumeModel.detailDriver
    property string detailScope: volumeModel.detailScope
    property string detailMountpoint: volumeModel.detailMountpoint
    property string detailCreated: volumeModel.detailCreatedText
    property double detailBytes: volumeModel.detailSizeBytes
    property bool detailSizeKnown: volumeModel.detailSizeKnown
    property string detailSize: volumeModel.detailSizeText
    property string detailReferences: volumeModel.detailRefCountText
    property bool detailAnonymous: volumeModel.detailAnonymous
    property var detail: volumeModel.detail
    property var general: volumeModel.generalModel
    property var usage: volumeModel.usedByModel
    property var labels: volumeModel.labelModel
    property var options: volumeModel.optionModel
    property var pluginStatus: volumeModel.statusModel
    property int labelCount: volumeModel.labelCount
    property int optionCount: volumeModel.optionCount
    property int statusCount: volumeModel.statusCount
    property bool creating: volumeModel.creating
    property string createError: volumeModel.createErrorMessage
    property bool preparingRemove: volumeModel.removePreparationActive
    property string removing: volumeModel.removingVolumeName
    property string removeError: volumeModel.removeErrorMessage
    property bool preparingPrune: volumeModel.prunePreparationActive
    property bool pruning: volumeModel.pruning
    property var pruneCandidates: volumeModel.pruneCandidateModel
    property string pruneSize: volumeModel.pruneKnownSizeText
    property int pruneUnknown: volumeModel.pruneUnknownSizeCount
    property string pruneError: volumeModel.pruneErrorMessage
    property string exporting: volumeModel.exportingVolumeName
    property string exportStatus: volumeModel.exportStatus
    property string exportError: volumeModel.exportErrorMessage
    property string cloning: volumeModel.cloningSourceName
    property string cloneStatus: volumeModel.cloneStatus
    property string cloneError: volumeModel.cloneErrorMessage
    property bool zstd: volumeModel.zstdAvailable
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/VolumeModelApi.qml",
        Some(volume_model_api),
    );

    // AppController also crosses the CXX-Qt naming boundary. Bind every
    // multi-word property exactly as QML consumes it so snake_case regressions
    // cannot silently prevent connection-state delivery.
    let app_controller_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    AppController {
        id: appController
        onDockerChanged: (kind) => {}
    }
    property int dockerStatus: appController.dockerStatus
    property string dockerStatusText: appController.dockerStatusText
    property string dockerHost: appController.dockerHost
    property string engineInfo: appController.engineInfoJson
    property bool overviewLoading: appController.overviewLoading
    property string overview: appController.overviewJson
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/AppControllerApi.qml",
        Some(app_controller_api),
    );

    // Exercise the permanent detail panel with a selected-image loading state.
    let selected_image_page = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 520
    height: 700
    ListModel {
        id: imageModel
        property string state: "ready"
        property string errorKind: ""
        property int totalImageCount: 0
        property string totalSizeText: "0 B"
        property bool loading: false
        property string currentSortMode: "used_first"
        property string selectedImageId: "sha256:test"
        property bool detailLoading: true
        property string detailState: "loading"
        property string detailError: ""
        property string detailErrorKind: ""
        property bool exporting: false
        property var detail: null
        property var environmentModel: []
        property var labelModel: []
        property var usageModel: []
        property string removingImageId: ""
        property string removeErrorMessage: ""
        property bool pulling: false
        property string pullErrorMessage: ""
        property string pullStatus: ""
        property bool pullProgressKnown: false
        property real pullPercent: 0
        property string pullProgressText: ""
        property string exportStatus: ""
        property string exportBytesText: ""
        property string exportErrorMessage: ""
        function initialize() {}
        function setSortMode(mode) {}
        function setSearchQuery(query) {}
        function refresh() {}
        function selectImage(id) {}
        function removeImage(id, force, prune) {}
        function exportImage(id, path) {}
        function reloadSelectedImage() {}
        function setEnvironmentSearchQuery(query) {}
        function setEnvironmentSortAscending(ascending) {}
        function setLabelSearchQuery(query) {}
        function setLabelSortAscending(ascending) {}
        function pullImage(reference, platform, username, password, registry) {}
        function cancelPull() {}
        function cancelExport() {}
    }
    ImagesPage {
        anchors.fill: parent
        imagesModel: imageModel
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/SelectedImagePage.qml",
        Some(selected_image_page),
    );

    // Exercise the loaded detail data paths, including responsive rows,
    // expandable tags, environment, labels, and Used By delegates.
    let loaded_image_detail = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 920
    height: 900
    QtObject {
        id: imageModel
        property bool detailLoading: false
        property string detailState: "ready"
        property string detailError: ""
        property string detailErrorKind: ""
        property bool exporting: false
        property var detail: ({
            imageId: "sha256:abcdef",
            shortId: "abcdef",
            displayName: "ubuntu:24.04",
            repoTags: ["ubuntu:24.04", "ubuntu:latest", "ubuntu:focal"],
            tagsText: "ubuntu:24.04\nubuntu:latest\nubuntu:focal",
            createdText: "3 days ago",
            createdFullText: "Jul 22, 2026 12:40 UTC",
            sizeText: "1.2 GiB",
            platform: "linux/arm64/v8",
            architecture: "arm64",
            os: "linux",
            commandText: "[\"/bin/sh\"]",
            entrypointText: "—",
            workingDir: "/work",
            user: "1000",
            stopSignal: "SIGTERM"
        })
        property var environmentRows: [{ key: "TOKEN", value: "a=b" }]
        property var labelRows: [{ key: "org.example", value: "yes" }]
        property var environmentModel: environmentRows
        property var labelModel: labelRows
        property var usageModel: [{
            containerId: "container-full-id",
            shortId: "container123",
            name: "floatctf-dev",
            state: "exited",
            status: "Exited (0)",
            createdText: "Jul 22, 2026 12:40 UTC"
        }]
        function setEnvironmentSearchQuery(query) {}
        function setEnvironmentSortAscending(ascending) {}
        function setLabelSearchQuery(query) {}
        function setLabelSortAscending(ascending) {}
        function reloadSelectedImage() {}
    }
    ImageDetailPanel {
        anchors.fill: parent
        imagesModel: imageModel
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/LoadedImageDetail.qml",
        Some(loaded_image_detail),
    );

    let selected_network_page = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 760
    height: 700
    ListModel {
        id: networkModel
        property string state: "ready"
        property string statusText: ""
        property string errorMessage: ""
        property string errorKind: ""
        property bool loading: false
        property int count: 1
        property int totalNetworkCount: 1
        property string currentSortMode: "name_asc"
        property string selectedNetworkId: "network-full-id"
        property string detailState: "loading"
        property string detailError: ""
        property var detail: null
        property var optionRows: []
        property var labelRows: []
        property var subnetRows: []
        property var containerRows: []
        property bool operationInProgress: false
        property bool creating: false
        property string createErrorMessage: ""
        property string removingNetworkId: ""
        property string removeErrorMessage: ""
        ListElement {
            networkId: "network-full-id"; shortId: "network1234"; name: "bridge"
            subnet: "172.17.0.0/16"; gateway: "172.17.0.1"; driver: "bridge"
            scope: "local"; createdAt: "2026-07-22T12:40:00Z"; createdText: "3 days ago"
            internal: false; attachable: false; ingress: false; ipv4: true; ipv6: false
            selected: true; busy: false; operation: ""
        }
        function initialize() {}
        function refresh() {}
        function setSearchQuery(query) {}
        function setSortMode(mode) {}
        function selectNetwork(id) {}
        function reloadSelectedNetwork() {}
        function prepareRemoveNetwork(id) {}
        function removeNetwork(id) {}
        function createNetwork(name, driver, subnet, gateway, flags, labels) {}
    }
    NetworksPage { anchors.fill: parent; networksModel: networkModel }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/SelectedNetworkPage.qml",
        Some(selected_network_page),
    );

    let loaded_network_detail = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 920
    height: 900
    QtObject {
        id: networkModel
        property string selectedNetworkId: "network-full-id"
        property string detailState: "ready"
        property string detailError: ""
        property var detail: ({
            networkId: "network-full-id", shortId: "network1234", name: "dev-network",
            createdText: "3 days ago", createdFullText: "Jul 22, 2026 12:40 UTC",
            driver: "bridge", scope: "local", subnet: "172.30.0.0/16",
            gateway: "172.30.0.1", internal: false, attachable: true,
            ingress: false, ipv4: true, ipv6: true, ipamDriver: "default",
            containerCount: 1
        })
        property var optionRows: [{ key: "com.docker.network.bridge.name", value: "br-dev" }]
        property var labelRows: [{ key: "environment", value: "development" }]
        property var subnetRows: [{ subnet: "172.30.0.0/16", gateway: "172.30.0.1", ipRange: "—" }]
        property var containerRows: [{
            containerId: "container-full-id", shortId: "container123", name: "web",
            endpointId: "endpoint-full-id", ipv4Address: "172.30.0.2/16",
            ipv6Address: "—", macAddress: "02:42:ac:1e:00:02"
        }]
        function reloadSelectedNetwork() {}
    }
    NetworkDetailPanel { anchors.fill: parent; networksModel: networkModel }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/LoadedNetworkDetail.qml",
        Some(loaded_network_detail),
    );

    // Exercise the permanent volume detail panel while a selected volume is
    // loading. The fake implements the same camelCase API consumed by QML.
    let selected_volume_page = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 760
    height: 760
    ListModel {
        id: volumeModel
        property string searchQuery: ""
        property string sortMode: "in_use_first"
        property string listState: "ready"
        property string errorMessage: ""
        property bool loading: false
        property int volumeCount: 1
        property int inUseCount: 1
        property int unusedCount: 0
        property string knownTotalSizeText: "1.0 GiB"
        property int knownSizeCount: 1
        property int unknownSizeCount: 0
        property string selectedVolumeName: "postgres-data"
        property string detailState: "loading"
        property string detailError: ""
        property string detailName: ""
        property string detailDriver: ""
        property string detailScope: ""
        property string detailMountpoint: ""
        property string detailCreatedText: ""
        property string detailSizeText: ""
        property string detailRefCountText: ""
        property bool detailAnonymous: false
        property bool selectedVolumeBusy: false
        property var usedByModel: []
        property var labelModel: []
        property var optionModel: []
        property var statusModel: []
        property int labelCount: 0
        property int optionCount: 0
        property int statusCount: 0
        property bool globalOperationInProgress: false
        property bool creating: false
        property string createErrorMessage: ""
        property string removingVolumeName: ""
        property string removeErrorMessage: ""
        property bool pruning: false
        property var pruneCandidateModel: []
        property string pruneKnownSizeText: "0 B"
        property int pruneUnknownSizeCount: 0
        property string pruneErrorMessage: ""
        property string exportingVolumeName: ""
        property string exportStatus: ""
        property string exportErrorMessage: ""
        property string cloningSourceName: ""
        property string cloneStatus: ""
        property string cloneErrorMessage: ""
        property bool zstdAvailable: false
        ListElement {
            volumeName: "postgres-data"; displayName: "postgres-data"; driver: "local"
            scope: "local"; mountpoint: "/var/lib/docker/volumes/postgres-data/_data"
            sizeBytes: 1073741824; sizeKnown: true; sizeText: "1.0 GiB"
            createdAt: "2026-07-22T12:40:00Z"; createdText: "3 days ago"
            inUse: true; usedByCount: 1; anonymous: false; selected: true
            busy: false; operation: ""; section: "in_use"
        }
        function initialize() {}
        function refresh() {}
        function setSearchQuery(query) { searchQuery = query }
        function setSortMode(mode) { sortMode = mode }
        function selectVolume(name) { selectedVolumeName = name }
        function reloadSelectedVolume() {}
        function prepareRemoveVolume(name) {}
        function preparePruneVolumes() {}
        function createVolume(name, driver, options, labels) {}
        function cancelCreate() {}
        function removeVolume(name, force) {}
        function pruneVolumes() {}
        function cancelPrune() {}
        function exportVolume(name, destination, format) {}
        function cancelExport() {}
        function cloneVolume(source, target, driver, options, copyLabels, cleanupFailed) {}
        function cancelClone() {}
        function navigateToContainer(containerId) {}
        function setLabelSearchQuery(query) {}
        function setLabelSortAscending(ascending) {}
        function setOptionSearchQuery(query) {}
        function setOptionSortAscending(ascending) {}
        function setStatusSearchQuery(query) {}
        function setStatusSortAscending(ascending) {}
    }
    VolumesPage { anchors.fill: parent; volumesModel: volumeModel }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/SelectedVolumePage.qml",
        Some(selected_volume_page),
    );

    // Exercise every populated volume-detail section: general values, Used By,
    // labels, driver options, and plugin status.
    let loaded_volume_detail = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 920
    height: 900
    QtObject {
        id: volumeModel
        property string selectedVolumeName: "postgres-data"
        property string detailState: "ready"
        property string detailError: ""
        property string detailName: "postgres-data"
        property string detailDriver: "local"
        property string detailScope: "local"
        property string detailMountpoint: "/var/lib/docker/volumes/postgres-data/_data"
        property string detailCreatedText: "Jul 22, 2026 12:40 UTC"
        property string detailSizeText: "1.0 GiB"
        property string detailRefCountText: "1"
        property bool detailAnonymous: false
        property bool selectedVolumeBusy: false
        property var usedByModel: [{
            containerId: "container-full-id", shortId: "container123", name: "postgres",
            state: "running", destination: "/var/lib/postgresql/data",
            readOnly: false, accessText: "Read/Write", propagation: "rprivate"
        }]
        property var labelModel: [{ key: "com.example.environment", value: "development" }]
        property var optionModel: [{ key: "type", value: "nfs" }, { key: "device", value: ":/exports/data" }]
        property var statusModel: [{ key: "availability", value: "online" }]
        property int labelCount: 1
        property int optionCount: 2
        property int statusCount: 1
        function reloadSelectedVolume() {}
        function setLabelSearchQuery(query) {}
        function setLabelSortAscending(ascending) {}
        function setOptionSearchQuery(query) {}
        function setOptionSortAscending(ascending) {}
        function setStatusSearchQuery(query) {}
        function setStatusSortAscending(ascending) {}
    }
    QtObject {
        id: filesModel
        property string filesState: "ready"
        property string errorMessage: ""
        property string errorKind: ""
        property string volumeName: "postgres-data"
        property string currentPath: "/"
        property bool canGoBack: false
        property bool canGoUp: false
        property bool showHidden: false
        property string searchQuery: ""
        property string sortColumn: "name"
        property bool sortDescending: false
        property bool directoriesFirst: true
        property string selectedEntryPath: ""
        property bool loading: false
        property int count: 2
        property bool truncated: false
        property bool active: true
        property var breadcrumbModel: [{ label: "postgres-data", path: "/" }]
        property bool previewLoading: false
        property string previewName: ""
        property string previewPath: ""
        property string previewKind: ""
        property string previewText: ""
        property string previewMime: ""
        property string previewSizeText: ""
        property bool previewTruncated: false
        property bool previewIsImage: false
        property bool previewIsText: false
        property bool previewIsBinary: false
        property string previewParseError: ""
        property string previewImagePath: ""
        property string previewError: ""
        property bool downloadInProgress: false
        property var propertiesModel: []
        // ListModel-like roles for the file table
        property var modelData: []
        function setActive(active) {}
        function openVolume(name) {}
        function closeVolume() {}
        function refresh() {}
        function openEntry(path) {}
        function goBack() {}
        function goUp() {}
        function navigateTo(path) {}
        function setSearchQuery(query) {}
        function setShowHidden(show) {}
        function setSort(column, descending) {}
        function toggleSort(column) {}
        function selectEntry(path) {}
        function previewEntry(path) {}
        function cancelPreview() {}
        function downloadEntry(path, destination) {}
        function cancelDownload() {}
        function loadProperties(path) {}
        function retry() {}
        function shutdown() {}
        // QAbstractListModel surface used by ListView
        function rowCount() { return 2 }
        function data(index, role) { return "" }
    }
    // Files tab selected so VolumeFilesView layout is exercised.
    VolumeDetailPanel {
        anchors.fill: parent
        volumesModel: volumeModel
        filesModel: filesModel
        detailTabIndex: 1
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/LoadedVolumeDetail.qml",
        Some(loaded_volume_detail),
    );

    // VolumeFilesView state overlays: loading / empty / error must load without
    // the old multi-fillHeight layout collapse.
    let volume_files_states = r#"
import QtQuick
import QtQuick.Layouts
import org.tuxstack.app
Item {
    width: 920
    height: 720
    ColumnLayout {
        anchors.fill: parent
        VolumeFilesView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            filesModel: QtObject {
                property string filesState: "starting"
                property string errorMessage: ""
                property bool canGoBack: false
                property bool canGoUp: false
                property bool showHidden: false
                property string searchQuery: ""
                property string sortColumn: "name"
                property bool sortDescending: false
                property string selectedEntryPath: ""
                property int count: 0
                property bool truncated: false
                property var breadcrumbModel: [{ label: "vol", path: "/" }]
                property bool previewLoading: false
                property string previewName: ""
                property string previewPath: ""
                property bool previewIsText: false
                property bool previewIsImage: false
                property bool previewIsBinary: false
                property string previewText: ""
                property string previewMime: ""
                property string previewSizeText: ""
                property bool previewTruncated: false
                property string previewParseError: ""
                property string previewImagePath: ""
                property var propertiesModel: []
                function setActive(a) {}
                function openVolume(n) {}
                function closeVolume() {}
                function refresh() {}
                function openEntry(p) {}
                function goBack() {}
                function goUp() {}
                function navigateTo(p) {}
                function setSearchQuery(q) {}
                function setShowHidden(s) {}
                function toggleSort(c) {}
                function selectEntry(p) {}
                function loadProperties(p) {}
                function retry() {}
                function cancelPreview() {}
                function downloadEntry(p, d) {}
            }
        }
        VolumeFilesView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            filesModel: QtObject {
                property string filesState: "empty"
                property string errorMessage: ""
                property bool canGoBack: false
                property bool canGoUp: true
                property bool showHidden: false
                property string searchQuery: ""
                property string sortColumn: "name"
                property bool sortDescending: false
                property string selectedEntryPath: ""
                property int count: 0
                property bool truncated: false
                property var breadcrumbModel: [{ label: "vol", path: "/" }, { label: "empty", path: "/empty" }]
                property bool previewLoading: false
                property string previewName: ""
                property string previewPath: ""
                property bool previewIsText: false
                property bool previewIsImage: false
                property bool previewIsBinary: false
                property string previewText: ""
                property string previewMime: ""
                property string previewSizeText: ""
                property bool previewTruncated: false
                property string previewParseError: ""
                property string previewImagePath: ""
                property var propertiesModel: []
                function setActive(a) {}
                function openVolume(n) {}
                function closeVolume() {}
                function refresh() {}
                function openEntry(p) {}
                function goBack() {}
                function goUp() {}
                function navigateTo(p) {}
                function setSearchQuery(q) {}
                function setShowHidden(s) {}
                function toggleSort(c) {}
                function selectEntry(p) {}
                function loadProperties(p) {}
                function retry() {}
                function cancelPreview() {}
                function downloadEntry(p, d) {}
            }
        }
        VolumeFilesView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            filesModel: QtObject {
                property string filesState: "error"
                property string errorMessage: "Permission denied while listing the folder."
                property bool canGoBack: false
                property bool canGoUp: false
                property bool showHidden: false
                property string searchQuery: ""
                property string sortColumn: "name"
                property bool sortDescending: false
                property string selectedEntryPath: ""
                property int count: 0
                property bool truncated: false
                property var breadcrumbModel: [{ label: "vol", path: "/" }]
                property bool previewLoading: false
                property string previewName: ""
                property string previewPath: ""
                property bool previewIsText: false
                property bool previewIsImage: false
                property bool previewIsBinary: false
                property string previewText: ""
                property string previewMime: ""
                property string previewSizeText: ""
                property bool previewTruncated: false
                property string previewParseError: ""
                property string previewImagePath: ""
                property var propertiesModel: []
                function setActive(a) {}
                function openVolume(n) {}
                function closeVolume() {}
                function refresh() {}
                function openEntry(p) {}
                function goBack() {}
                function goUp() {}
                function navigateTo(p) {}
                function setSearchQuery(q) {}
                function setShowHidden(s) {}
                function toggleSort(c) {}
                function selectEntry(p) {}
                function loadProperties(p) {}
                function retry() {}
                function cancelPreview() {}
                function downloadEntry(p, d) {}
            }
        }
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/VolumeFilesStates.qml",
        Some(volume_files_states),
    );

    // ImageFilesView reuses the volume browser layout but surfaces the
    // image "unsupported" state and image-specific strings.
    let image_files_states = r#"
import QtQuick
import QtQuick.Layouts
import org.tuxstack.app
Item {
    width: 920
    height: 720
    ColumnLayout {
        anchors.fill: parent
        ImageFilesView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            filesModel: QtObject {
                property string filesState: "unsupported"
                property string errorMessage: ""
                property bool canGoBack: false
                property bool canGoUp: false
                property bool showHidden: false
                property string searchQuery: ""
                property string sortColumn: "name"
                property bool sortDescending: false
                property string selectedEntryPath: ""
                property int count: 0
                property bool truncated: false
                property var breadcrumbModel: [{ label: "sha256:abcdef", path: "/" }]
                property bool previewLoading: false
                property string previewName: ""
                property string previewPath: ""
                property bool previewIsText: false
                property bool previewIsImage: false
                property bool previewIsBinary: false
                property string previewText: ""
                property string previewMime: ""
                property string previewSizeText: ""
                property bool previewTruncated: false
                property string previewParseError: ""
                property string previewImagePath: ""
                property var propertiesModel: []
                function setActive(a) {}
                function openImage(i) {}
                function closeImage() {}
                function refresh() {}
                function openEntry(p) {}
                function goBack() {}
                function goUp() {}
                function navigateTo(p) {}
                function setSearchQuery(q) {}
                function setShowHidden(s) {}
                function toggleSort(c) {}
                function selectEntry(p) {}
                function loadProperties(p) {}
                function retry() {}
                function cancelPreview() {}
                function downloadEntry(p, d) {}
            }
        }
        ImageFilesView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            filesModel: QtObject {
                property string filesState: "ready"
                property string errorMessage: ""
                property bool canGoBack: true
                property bool canGoUp: true
                property bool showHidden: false
                property string searchQuery: ""
                property string sortColumn: "name"
                property bool sortDescending: false
                property string selectedEntryPath: "/etc"
                property int count: 2
                property bool truncated: false
                property var breadcrumbModel: [{ label: "ubuntu:24.04", path: "/" }, { label: "etc", path: "/etc" }]
                property bool previewLoading: false
                property string previewName: ""
                property string previewPath: ""
                property bool previewIsText: false
                property bool previewIsImage: false
                property bool previewIsBinary: false
                property string previewText: ""
                property string previewMime: ""
                property string previewSizeText: ""
                property bool previewTruncated: false
                property string previewParseError: ""
                property string previewImagePath: ""
                property var propertiesModel: []
                function setActive(a) {}
                function openImage(i) {}
                function closeImage() {}
                function refresh() {}
                function openEntry(p) {}
                function goBack() {}
                function goUp() {}
                function navigateTo(p) {}
                function setSearchQuery(q) {}
                function setShowHidden(s) {}
                function toggleSort(c) {}
                function selectEntry(p) {}
                function loadProperties(p) {}
                function retry() {}
                function cancelPreview() {}
                function downloadEntry(p, d) {}
            }
        }
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/ImageFilesStates.qml",
        Some(image_files_states),
    );

    // Exercise the Image Files tab inside the detail panel: a selected image
    // with a ready fake files model.
    let image_files_tab = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 920
    height: 900
    QtObject {
        id: imageModel
        property bool detailLoading: false
        property string detailState: "ready"
        property string detailError: ""
        property string detailErrorKind: ""
        property bool exporting: false
        property string selectedImageId: "sha256:abcdef"
        property var detail: ({
            imageId: "sha256:abcdef", shortId: "abcdef",
            displayName: "ubuntu:24.04", repoTags: ["ubuntu:24.04"],
            tagsText: "ubuntu:24.04", createdText: "3 days ago",
            createdFullText: "Jul 22, 2026 12:40 UTC", sizeText: "1.2 GiB",
            platform: "linux/amd64", architecture: "amd64", os: "linux",
            commandText: "[\"/bin/sh\"]", entrypointText: "—",
            workingDir: "/", user: "root", stopSignal: "SIGTERM"
        })
        property var environmentRows: []
        property var labelRows: []
        property var environmentModel: []
        property var labelModel: []
        property var usageModel: []
        function setEnvironmentSearchQuery(query) {}
        function setEnvironmentSortAscending(ascending) {}
        function setLabelSearchQuery(query) {}
        function setLabelSortAscending(ascending) {}
        function reloadSelectedImage() {}
    }
    QtObject {
        id: filesModel
        property string filesState: "ready"
        property string errorMessage: ""
        property string errorKind: ""
        property string imageId: "sha256:abcdef"
        property string currentPath: "/"
        property bool canGoBack: false
        property bool canGoUp: false
        property bool showHidden: false
        property string searchQuery: ""
        property string sortColumn: "name"
        property bool sortDescending: false
        property bool directoriesFirst: true
        property string selectedEntryPath: ""
        property bool loading: false
        property int count: 2
        property bool truncated: false
        property bool active: true
        property var breadcrumbModel: [{ label: "sha256:abcdef", path: "/" }]
        property bool previewLoading: false
        property string previewName: ""
        property string previewPath: ""
        property string previewKind: ""
        property string previewText: ""
        property string previewMime: ""
        property string previewSizeText: ""
        property bool previewTruncated: false
        property bool previewIsImage: false
        property bool previewIsText: false
        property bool previewIsBinary: false
        property string previewParseError: ""
        property string previewImagePath: ""
        property string previewError: ""
        property bool downloadInProgress: false
        property var propertiesModel: []
        function setActive(active) {}
        function openImage(imageId) {}
        function closeImage() {}
        function refresh() {}
        function openEntry(path) {}
        function goBack() {}
        function goUp() {}
        function navigateTo(path) {}
        function setSearchQuery(query) {}
        function setShowHidden(show) {}
        function setSort(column, descending) {}
        function toggleSort(column) {}
        function selectEntry(path) {}
        function previewEntry(path) {}
        function cancelPreview() {}
        function downloadEntry(path, destination) {}
        function cancelDownload() {}
        function loadProperties(path) {}
        function retry() {}
        function rowCount() { return 2 }
        function data(index, role) { return "" }
    }
    // Files tab selected so ImageFilesView layout is exercised.
    ImageDetailPanel {
        anchors.fill: parent
        imagesModel: imageModel
        filesModel: filesModel
        detailTabIndex: 1
    }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/ImageFilesTab.qml",
        Some(image_files_tab),
    );

    // Keep every operation dialog alive against populated state, including a
    // prune candidate and in-progress/error branches used by dialog bindings.
    let populated_volume_dialogs = r#"
import QtQuick
import org.tuxstack.app
Item {
    width: 920
    height: 900
    QtObject {
        id: volumeModel
        property bool creating: true
        property string createErrorMessage: "create fixture error"
        property string detailDriver: "local"
        property string removingVolumeName: "postgres-data"
        property string removeErrorMessage: "remove fixture error"
        property bool pruning: true
        property var pruneCandidateModel: [{ volumeName: "unused-cache", sizeText: "Unknown" }]
        property string pruneKnownSizeText: "128.0 MiB"
        property int pruneUnknownSizeCount: 1
        property string pruneErrorMessage: "prune fixture error"
        property string exportingVolumeName: "postgres-data"
        property string exportStatus: "Writing archive…"
        property string exportErrorMessage: "export fixture error"
        property string cloningSourceName: "postgres-data"
        property string cloneStatus: "Copying volume data…"
        property string cloneErrorMessage: "clone fixture error"
        property bool zstdAvailable: true
        function createVolume(name, driver, options, labels) {}
        function cancelCreate() {}
        function removeVolume(name, force) {}
        function pruneVolumes() {}
        function cancelPrune() {}
        function exportVolume(name, destination, format) {}
        function cancelExport() {}
        function cloneVolume(source, target, driver, options, copyLabels, cleanupFailed) {}
        function cancelClone() {}
    }
    CreateVolumeDialog { volumesModel: volumeModel }
    RemoveVolumeDialog {
        volumesModel: volumeModel
        volumeName: "postgres-data"
        driver: "local"
        sizeText: "1.0 GiB"
        usedByCount: 1
        mountpoint: "/var/lib/docker/volumes/postgres-data/_data"
        submitted: true
    }
    PruneVolumesDialog { volumesModel: volumeModel; submitted: true }
    ExportVolumeDialog { volumesModel: volumeModel; volumeName: "postgres-data"; submitted: true }
    CloneVolumeDialog { volumesModel: volumeModel; sourceVolume: "postgres-data"; submitted: true }
}
"#;
    assert_qml_source_loads(
        "qrc:/qt/qml/org/tuxstack/app/tests/PopulatedVolumeDialogs.qml",
        Some(populated_volume_dialogs),
    );

    assert_qml_loads(&format!("{base}/Main.qml"));

    drop(app);
}


#[test]
fn image_detail_tabs_render_text_at_runtime() {
    let _qt_guard = QT_GUI_LOCK.lock().unwrap();
    unsafe {
        std::env::set_var("QT_QPA_PLATFORM", "offscreen");
    }
    crate::runtime::init();
    let app = QGuiApplication::new();
    assert!(!app.is_null());

    let source = r#"
import QtQuick
import QtQuick.Controls as QQC2
import org.tuxstack.app
Item {
    id: host
    property QtObject fakeImages: QtObject {
        property string selectedImageId: "sha256:abc"
        property string detailState: "ready"
        property var detail: null
        function reloadSelectedImage() {}
    }
    ImageDetailPanel {
        id: panel
        anchors.fill: parent
        imagesModel: host.fakeImages
        filesModel: null
    }
    Component.onCompleted: {
        let texts = []
        function walk(o) {
            for (let i = 0; i < o.children.length; i++) {
                const c = o.children[i]
                if (c instanceof QQC2.TabBar) {
                    for (let j = 0; j < c.contentChildren.length; j++) {
                        // Reading .text forces lazy binding evaluation, which
                        // is where an unresolved I18n would surface.
                        texts.push(String(c.contentChildren[j].text))
                    }
                }
                walk(c)
            }
        }
        walk(panel)
        host.objectName = "TABTEXT:" + texts.join("|")
    }
}
"#;
    let mut engine = QQmlApplicationEngine::new();
    let text = Arc::new(std::sync::Mutex::new(String::new()));
    let captured = text.clone();
    if let Some(mut engine) = engine.as_mut() {
        {
            let qml_engine: Pin<&mut QQmlEngine> = engine.as_mut().upcast_pin();
            qml_engine.set_output_warnings_to_standard_error(true);
        }
        engine
            .as_mut()
            .on_object_created(move |_, object, _| {
                if object.is_null() {
                    return;
                }
                let qobject: &cxx_qt::QObject = unsafe { &*object };
                use cxx_qt_lib::QObjectExt;
                let name = qobject.object_name().to_string();
                if name.starts_with("TABTEXT:") {
                    *captured.lock().unwrap() = name;
                }
            })
            .release();
        engine.load_data(
            &QByteArray::from(source),
            &QUrl::from("qrc:/qt/qml/org/tuxstack/app/tests/ImageDetailTabsText.qml"),
        );
    }
    let result = text.lock().unwrap().clone();
    eprintln!("runtime tab texts: {result:?}");
    assert!(
        result.starts_with("TABTEXT:Info|Files"),
        "Image detail tabs must render 'Info' and 'Files' text; got {result:?}. \
         This usually means the I18n singleton is not in scope (missing `import org.tuxstack.app`)."
    );
}

