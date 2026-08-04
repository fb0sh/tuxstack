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
    property string pendingContainerId: ""
    property bool controllerInitialized: false

    signal notificationRequested(string message)
    signal retryConnectionRequested()
    signal volumeNavigationRequested(string volumeName)
    signal networkNavigationRequested(string networkId, string networkName)

    title: I18n.i18nd("tuxstack", "Containers")
    padding: 0

    function initializeController() {
        if (!root.containersModel || root.controllerInitialized)
            return
        root.controllerInitialized = true
        root.containersModel.initialize()
        if (root.pendingContainerId.length > 0)
            root.containersModel.selectContainer(root.pendingContainerId)
    }

    Component.onCompleted: root.initializeController()
    onContainersModelChanged: root.initializeController()
    onPendingContainerIdChanged: {
        if (root.containersModel && root.pendingContainerId.length > 0)
            root.containersModel.selectContainer(root.pendingContainerId)
    }

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
            // Live-tab capabilities stay false until the corresponding real
            // controllers are registered; no non-functional menu items appear.
            logsCapability: false
            terminalCapability: false
            filesCapability: false

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
            onRetryRequested: if (root.containersModel) root.containersModel.reloadDetail()
            onContainerRequested: function(id) {
                if (root.containersModel) root.containersModel.selectContainer(id)
            }
            onBrowserRequested: function(url) { Qt.openUrlExternally(url) }
            onVolumeRequested: function(name) { root.volumeNavigationRequested(name) }
            onNetworkRequested: function(id, name) { root.networkNavigationRequested(id, name) }
            onHostPathRequested: function(path) { Qt.openUrlExternally("file://" + encodeURIComponent(path).replace(/%2F/g, "/")) }
            onProjectFolderRequested: function(path) { Qt.openUrlExternally("file://" + encodeURIComponent(path).replace(/%2F/g, "/")) }
        }
    }

    RemoveContainerDialog { id: removeDialog; containersModel: root.containersModel }
    RemoveContainerGroupDialog { id: removeGroupDialog; containersModel: root.containersModel }
    KillContainerDialog { id: killDialog; containersModel: root.containersModel }
    RenameContainerDialog { id: renameDialog; containersModel: root.containersModel }

    Connections {
        target: root.containersModel
        ignoreUnknownSignals: true

        function onRemoveContainerPrepared(id, name, image, state, composeProject, mounts) {
            removeDialog.prepare(id, name, image, state, composeProject, mounts)
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
}
