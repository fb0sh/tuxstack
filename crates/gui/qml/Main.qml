import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * TuxStack application shell.
 *
 * The application shell owns the typed tuxstackd connection and long-lived
 * presentation models while the fixed StackLayout keeps page navigation
 * responsive and preserves page state.
 */
Kirigami.ApplicationWindow {
    id: root

    property string currentPage: "containers"
    property string pendingContainerId: ""
    property bool userCollapsed: false

    property string pendingDetailContainerId: ""
    property bool pendingDetailFallback: false

    function refreshThrottled(model, key) {
        // Trailing debounce: every request restarts the model's timer, so a
        // final daemon state is never discarded by a leading-edge throttle.
        switch (key) {
        case "images": imagesRefreshTimer.restart(); break
        case "containers": containersRefreshTimer.restart(); break
        case "volumes": volumesRefreshTimer.restart(); break
        case "networks": networksRefreshTimer.restart(); break
        }
    }

    function rootfsSnapshotAffected(action) {
        return ["start", "restart", "die", "stop", "kill", "destroy", "rename"]
                .indexOf(action) !== -1
    }

    function terminalAffected(action) {
        return ["stop", "die", "kill", "destroy", "restart", "pause"]
                .indexOf(action) !== -1
    }

    function scheduleSelectedDetail(actorId, fallback) {
        root.pendingDetailContainerId = actorId
        root.pendingDetailFallback = fallback
        containerDetailRefreshTimer.restart()
    }

    Timer {
        id: imagesRefreshTimer
        interval: 200
        onTriggered: imagesModel.refresh()
    }
    Timer {
        id: containersRefreshTimer
        interval: 200
        onTriggered: containersModel.refresh()
    }
    Timer {
        id: volumesRefreshTimer
        interval: 200
        onTriggered: volumesModel.refresh()
    }
    Timer {
        id: networksRefreshTimer
        interval: 200
        onTriggered: networksModel.refresh()
    }
    Timer {
        id: containerDetailRefreshTimer
        interval: 200
        onTriggered: {
            if (containersModel.selectionKind !== "container")
                return
            if (root.pendingDetailFallback
                    || containersModel.selectionId === root.pendingDetailContainerId)
                containersModel.reloadDetail()
            root.pendingDetailContainerId = ""
            root.pendingDetailFallback = false
        }
    }

    readonly property bool compactMode: width < Kirigami.Units.gridUnit * 45
    readonly property bool sidebarCollapsed: compactMode || userCollapsed

    title: qsTr("TuxStack — %1").arg(pageTitle(currentPage))
    width: Kirigami.Units.gridUnit * 61
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 24
    minimumHeight: Kirigami.Units.gridUnit * 24
    visible: true

    AppController {
        id: appController
    }

    ContainersListModel {
        id: containersModel
    }

    ContainerStatsModel {
        id: containerStatsModel
    }

    ContainerLogsModel {
        id: containerLogsModel
    }

    ContainerTerminalModel {
        id: containerTerminalModel
    }

    LocalFuseFileListModel {
        id: containerFilesModel
    }

    ImageListModel {
        id: imagesModel
    }

    NetworkListModel {
        id: networksModel
    }

    VolumeListModel {
        id: volumesModel
    }

    LocalFuseFileListModel {
        id: volumeFilesModel
    }

    LocalFuseFileListModel {
        id: imageFilesModel
    }

    Component.onCompleted: appController.startup()
    onClosing: {
        containersModel.shutdown()
        containerStatsModel.shutdown()
        containerLogsModel.shutdown()
        containerTerminalModel.shutdown()
        containerFilesModel.shutdown()
        imagesModel.shutdown()
        networksModel.shutdown()
        volumesModel.shutdown()
        volumeFilesModel.shutdown()
        imageFilesModel.shutdown()
    }

    Connections {
        target: appController

        function onDockerStatusChanged() {
            containersModel.setConnectionState(appController.dockerStatus,
                                                appController.dockerStatusText)
            containerFilesModel.setConnectionState(appController.dockerStatus)
            imagesModel.setConnectionState(appController.dockerStatus,
                                           appController.dockerStatusText)
            networksModel.setConnectionState(appController.dockerStatus,
                                             appController.dockerStatusText)
            volumesModel.setConnectionState(appController.dockerStatus,
                                            appController.dockerStatusText)
            volumeFilesModel.setConnectionState(appController.dockerStatus)
            imageFilesModel.setConnectionState(appController.dockerStatus)
        }

        function onDockerStatusTextChanged() {
            if (appController.dockerStatus !== 1) {
                containersModel.setConnectionState(appController.dockerStatus,
                                                    appController.dockerStatusText)
                containerFilesModel.setConnectionState(appController.dockerStatus)
                imagesModel.setConnectionState(appController.dockerStatus,
                                               appController.dockerStatusText)
                networksModel.setConnectionState(appController.dockerStatus,
                                                 appController.dockerStatusText)
                volumesModel.setConnectionState(appController.dockerStatus,
                                                appController.dockerStatusText)
                volumeFilesModel.setConnectionState(appController.dockerStatus)
                imageFilesModel.setConnectionState(appController.dockerStatus)
            }
        }

        function onDockerChanged(kind) {
            // Resource-level refreshes remain one signal per kind. Container
            // detail side effects are handled by onContainerChanged below.
            if (appController.dockerStatus !== 1) {
                return
            }
            switch (kind) {
            case "images":
                root.refreshThrottled(imagesModel, "images")
                break
            case "networks":
                root.refreshThrottled(networksModel, "networks")
                break
            case "volumes":
                root.refreshThrottled(volumesModel, "volumes")
                break
            case "daemon":
                appController.refreshOverview()
                break
            }
        }

        function onContainerChanged(actorId, action) {
            if (appController.dockerStatus !== 1)
                return

            root.refreshThrottled(containersModel, "containers")
            // Container mount associations can change without a volume
            // event, so every container batch also refreshes volumes.
            root.refreshThrottled(volumesModel, "volumes")

            // Empty IDs mark a reconnect, omitted actor ID, or bounded-batch
            // fallback: refresh selected detail because precise matching is
            // impossible, but do not invalidate unrelated tools.
            if (actorId.length === 0) {
                if (containersModel.selectionKind === "container")
                    root.scheduleSelectedDetail("", true)
                return
            }

            const selected = containersModel.selectionKind === "container"
                    && containersModel.selectionId === actorId
            if (selected) {
                root.scheduleSelectedDetail(actorId, false)
                if (root.rootfsSnapshotAffected(action))
                    containerFilesModel.invalidateSnapshot(actorId)
            }
            if (root.terminalAffected(action))
                containerTerminalModel.invalidateContainer(actorId)
        }
    }

    Connections {
        target: imagesModel

        function onContainerNavigationRequested(containerId) {
            root.navigateToContainer(containerId)
        }
    }

    Connections {
        target: volumesModel

        function onContainerNavigationRequested(containerId) {
            root.navigateToContainer(containerId)
        }
    }

    function navigateToContainer(containerId) {
        root.pendingContainerId = containerId
        root.currentPage = "containers"
    }

    function pageIndex(pageId) {
        switch (pageId) {
        case "containers": return 0
        case "images": return 1
        case "volumes": return 2
        case "networks": return 3
        case "activity": return 4
        case "commands": return 5
        case "devices": return 6
        case "settings": return 7
        default: return 0
        }
    }

    function pageTitle(pageId) {
        switch (pageId) {
        case "containers": return qsTr("Containers")
        case "images": return qsTr("Images")
        case "volumes": return qsTr("Volumes")
        case "networks": return qsTr("Networks")
        case "activity": return qsTr("Activity Monitor")
        case "commands": return qsTr("Commands")
        case "devices": return qsTr("Devices")
        case "settings": return qsTr("Settings")
        default: return qsTr("Containers")
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        AppSidebar {
            id: sidebar

            Layout.fillHeight: true
            Layout.minimumWidth: sidebar.implicitWidth
            Layout.preferredWidth: sidebar.implicitWidth
            Layout.maximumWidth: sidebar.implicitWidth
            currentPage: root.currentPage
            collapsed: root.sidebarCollapsed
            collapseEnabled: !root.compactMode

            onPageRequested: function(pageId) {
                root.currentPage = pageId
            }
            onCollapseRequested: {
                if (!root.compactMode)
                    root.userCollapsed = !root.userCollapsed
            }
        }

        Kirigami.Separator {
            Layout.fillHeight: true
        }

        StackLayout {
            id: pageStack

            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.pageIndex(root.currentPage)

            ContainersPage {
                containersModel: containersModel
                statsModel: containerStatsModel
                logsModel: containerLogsModel
                terminalModel: containerTerminalModel
                filesModel: containerFilesModel
                imagesModel: imagesModel
                networksModel: networksModel
                volumesModel: volumesModel
                pendingContainerId: root.pendingContainerId
                onNotificationRequested: function(message) {
                    root.showPassiveNotification(message)
                }
                onPendingContainerRequested: containerId => root.pendingContainerId = containerId
                onPendingContainerConsumed: function(containerId) {
                    if (root.pendingContainerId === containerId)
                        root.pendingContainerId = ""
                }
                onRetryConnectionRequested: appController.startup()
                onVolumeNavigationRequested: function(volumeName) {
                    root.currentPage = "volumes"
                    volumesModel.selectVolume(volumeName)
                }
                onNetworkNavigationRequested: function(networkId, networkName) {
                    root.currentPage = "networks"
                    if (networkId && networkId.length > 0)
                        networksModel.selectNetwork(networkId)
                }
            }
            ImagesPage {
                imagesModel: imagesModel
                filesModel: imageFilesModel
                onContainerNavigationRequested: function(containerId) {
                    imagesModel.requestContainerNavigation(containerId)
                }
                onNotificationRequested: function(message) {
                    root.showPassiveNotification(message)
                }
                onInitializationRequested: {
                    imagesModel.setConnectionState(appController.dockerStatus,
                                                   appController.dockerStatusText)
                }
                onRetryConnectionRequested: appController.startup()
            }
            VolumesPage {
                volumesModel: volumesModel
                filesModel: volumeFilesModel
                onNotificationRequested: function(message) {
                    root.showPassiveNotification(message)
                }
                onInitializationRequested: {
                    volumesModel.setConnectionState(appController.dockerStatus,
                                                    appController.dockerStatusText)
                }
                onRetryConnectionRequested: appController.startup()
            }
            NetworksPage {
                networksModel: networksModel
                onNotificationRequested: function(message) {
                    root.showPassiveNotification(message)
                }
                onInitializationRequested: {
                    networksModel.setConnectionState(appController.dockerStatus,
                                                     appController.dockerStatusText)
                }
                onRetryConnectionRequested: appController.startup()
            }
            ActivityMonitorPage { }
            CommandsPage { }
            DevicesPage { }
            SettingsPage { }
        }
    }
}
