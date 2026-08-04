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
        "components/ContainerActions.qml",
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
        "dialogs/CreateNetworkDialog.qml",
        "dialogs/RemoveNetworkDialog.qml",
        "dialogs/PullImageDialog.qml",
        "dialogs/RemoveImageDialog.qml",
        "dialogs/ExportImageDialog.qml",
        "dialogs/ConfirmRemoveDialog.qml",
        "dialogs/ContainerLogsDialog.qml",
        "dialogs/ContainerInspectDialog.qml",
        "dialogs/ErrorDetailsDialog.qml",
        "pages/OverviewPage.qml",
        "pages/ContainersPage.qml",
        "pages/ContainerDetailsPage.qml",
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
        "ContainerListModel",
        "ImageListModel",
        "NetworkListModel",
        "VolumeListModel",
        "ContainerDetailController",
        "LogListModel",
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

    // AppController also crosses the CXX-Qt naming boundary. Bind every
    // multi-word property exactly as QML consumes it so snake_case regressions
    // cannot silently prevent connection-state delivery.
    let app_controller_api = r#"
import QtQuick
import org.tuxstack.app
Item {
    AppController { id: appController }
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

    assert_qml_loads(&format!("{base}/Main.qml"));

    drop(app);
}
