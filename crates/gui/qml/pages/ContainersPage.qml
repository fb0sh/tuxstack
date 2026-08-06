pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components/containers"
import "../dialogs/containers"

Kirigami.Page {
    id: root

    property var containersModel: null
    property var statsModel: null
    property var logsModel: null
    property var terminalModel: null
    property var filesModel: null
    property var imagesModel: null
    property var networksModel: null
    property var volumesModel: null
    property string pendingContainerId: ""
    property bool controllerInitialized: false

    signal notificationRequested(string message)
    signal pendingContainerRequested(string containerId)
    signal pendingContainerConsumed(string containerId)
    signal retryConnectionRequested()
    signal startServiceRequested()
    signal volumeNavigationRequested(string volumeName)
    signal networkNavigationRequested(string networkId, string networkName)
    signal externalTerminalRequested(string containerId)

    title: I18n.i18nd("tuxstack", "Containers")
    padding: 0

    function applyPendingContainerSelection() {
        if (!root.containersModel || root.pendingContainerId.length === 0)
            return
        root.containersModel.selectContainer(root.pendingContainerId)
        if (root.containersModel.selectionKind === "container"
                && root.containersModel.selectionId === root.pendingContainerId)
            root.pendingContainerConsumed(root.pendingContainerId)
    }

    function initializeController() {
        if (!root.containersModel || root.controllerInitialized)
            return
        root.controllerInitialized = true
        root.containersModel.initialize()
        root.applyPendingContainerSelection()
    }

    Component.onCompleted: root.initializeController()
    onContainersModelChanged: root.initializeController()
    onPendingContainerIdChanged: root.applyPendingContainerSelection()

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ContainerListPanel {
            id: listPanel
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 16
            Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 18,
                                            Math.min(Kirigami.Units.gridUnit * 22,
                                                     root.width * 0.38))
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            containersModel: root.containersModel
            logsCapability: root.logsModel !== null
            terminalCapability: root.terminalModel !== null
            filesCapability: root.filesModel !== null

            onCreateRequested: createDialog.prepare()
            onRemoveContainerRequested: function(id) {
                // The bridge emits the complete real-time inspect summary.
                root.containersModel.prepareRemoveContainer(id)
            }
            onRemoveGroupRequested: function(id) {
                root.containersModel.prepareRemoveGroup(id)
            }
            onRenameContainerRequested: function(id) {
                const currentName = root.containersModel.selectionId === id
                                  ? root.containersModel.detailName : id
                renameDialog.prepare(id, currentName)
            }
            onKillContainerRequested: function(id) {
                const currentName = root.containersModel.selectionId === id
                                  ? root.containersModel.detailName : id
                killDialog.prepare(id, currentName)
            }
            onLogsRequested: id => detailPanel.openContainerTab(id, "logs")
            onTerminalRequested: id => root.externalTerminalRequested(id)
            onInAppTerminalRequested: id => detailPanel.openContainerTab(id, "terminal")
            onNotificationRequested: message => root.notificationRequested(message)
            onFilesRequested: id => detailPanel.openContainerTab(id, "files")
        }

        Kirigami.Separator { Layout.fillHeight: true }

        // The detail column is permanent. With no selection its child content
        // is entirely blank, preserving the three-column geometry.
        ContainerDetailPanel {
            id: detailPanel
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            containersModel: root.containersModel
            statsModel: root.statsModel
            logsModel: root.logsModel
            terminalModel: root.terminalModel
            filesModel: root.filesModel
            localEndpoint: root.containersModel ? root.containersModel.localEndpoint : false
            pageVisible: root.visible
            onRetryRequested: if (root.containersModel) root.containersModel.reloadDetail()
            onContainerRequested: function(id) {
                if (root.containersModel) root.containersModel.selectContainer(id)
            }
            onBrowserRequested: function(url) { Qt.openUrlExternally(url) }
            onVolumeRequested: function(name) { root.volumeNavigationRequested(name) }
            onNetworkRequested: function(id, name) { root.networkNavigationRequested(id, name) }
            onHostPathRequested: function(path) { Qt.openUrlExternally("file://" + encodeURIComponent(path).replace(/%2F/g, "/")) }
            onNotificationRequested: message => root.notificationRequested(message)
        }
    }

    CreateContainerDialog {
        id: createDialog
        containersModel: root.containersModel
        imagesModel: root.imagesModel
        networksModel: root.networksModel
        volumesModel: root.volumesModel
    }
    RemoveContainerDialog { id: removeDialog; containersModel: root.containersModel }
    RemoveContainerGroupDialog { id: removeGroupDialog; containersModel: root.containersModel }
    KillContainerDialog { id: killDialog; containersModel: root.containersModel }
    RenameContainerDialog { id: renameDialog; containersModel: root.containersModel }

    Connections {
        target: root.containersModel
        ignoreUnknownSignals: true

        function onCountChanged() { root.applyPendingContainerSelection() }
        function onListStateChanged() { root.applyPendingContainerSelection() }
        function onContainerCreated(containerId, started, message) {
            createDialog.close()
            root.pendingContainerRequested(containerId)
            root.notificationRequested(message)
        }
        function onRemoveContainerPrepared(preparation) {
            removeDialog.prepare(String(preparation.id || ""),
                                 String(preparation.name || ""),
                                 String(preparation.image || ""),
                                 String(preparation.state || ""),
                                 String(preparation.composeProject || ""),
                                 preparation.mounts || [])
        }
        function onRemoveContainerPreparationFailed(id, message) {
            root.notificationRequested(message)
        }
        function onRemoveGroupPrepared(id, projectName, targets) {
            removeGroupDialog.prepare(id, projectName, targets)
        }
        function onOperationFinished(operation, id, success, message) {
            root.notificationRequested(message)
        }
        function onBrowserUrlRequested(url) { Qt.openUrlExternally(url) }
        function onVolumeNavigationRequested(volumeName) {
            root.volumeNavigationRequested(volumeName)
        }
        function onNetworkNavigationRequested(networkId, networkName) {
            root.networkNavigationRequested(networkId, networkName)
        }
        function onHostPathRequested(path) {
            Qt.openUrlExternally("file://" + encodeURIComponent(path).replace(/%2F/g, "/"))
        }
    }

    Connections {
        target: root.filesModel
        ignoreUnknownSignals: true

        function onStartServiceRequested() { root.startServiceRequested() }
    }
}
