import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * TuxStack application shell.
 *
 * The application shell owns the Docker connection and long-lived resource
 * models while the fixed StackLayout keeps page navigation responsive and
 * preserves page state.
 */
Kirigami.ApplicationWindow {
    id: root

    property string currentPage: "containers"
    property string pendingContainerId: ""
    property bool userCollapsed: false

    property var lastRefreshAt: ({}) // model key -> timestamp

    function refreshThrottled(model, key) {
        // A single debounced event burst can carry both Containers and
        // Volumes kinds; both map to volumesModel. Refresh each model at
        // most once per burst so the list does not rebuild twice.
        const now = Date.now()
        const last = root.lastRefreshAt[key]
        if (last !== undefined && now - last < 600) {
            return
        }
        root.lastRefreshAt[key] = now
        model.refresh()
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

    ImageListModel {
        id: imagesModel
    }

    NetworkListModel {
        id: networksModel
    }

    VolumeListModel {
        id: volumesModel
    }

    VolumeFileListModel {
        id: volumeFilesModel
    }

    Component.onCompleted: appController.startup()
    onClosing: {
        imagesModel.shutdown()
        networksModel.shutdown()
        volumesModel.shutdown()
        volumeFilesModel.shutdown()
    }

    Connections {
        target: appController

        function onDockerStatusChanged() {
            imagesModel.setConnectionState(appController.dockerStatus,
                                           appController.dockerStatusText)
            networksModel.setConnectionState(appController.dockerStatus,
                                             appController.dockerStatusText)
            volumesModel.setConnectionState(appController.dockerStatus,
                                            appController.dockerStatusText)
            volumeFilesModel.setConnectionState(appController.dockerStatus,
                                                appController.dockerStatusText)
        }

        function onDockerStatusTextChanged() {
            if (appController.dockerStatus !== 1) {
                imagesModel.setConnectionState(appController.dockerStatus,
                                               appController.dockerStatusText)
                networksModel.setConnectionState(appController.dockerStatus,
                                                 appController.dockerStatusText)
                volumesModel.setConnectionState(appController.dockerStatus,
                                                appController.dockerStatusText)
                volumeFilesModel.setConnectionState(appController.dockerStatus,
                                                    appController.dockerStatusText)
            }
        }

        function onDockerChanged(kind) {
            // Event-driven targeted refresh: only the affected model reloads.
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
                // Container mounts affect volume usage association too, so
                // both kinds refresh volumesModel.
                root.refreshThrottled(volumesModel, "volumes")
                break
            case "containers":
                root.refreshThrottled(volumesModel, "volumes")
                break
            case "daemon":
                appController.refreshOverview()
                break
            }
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

            ContainersPage { }
            ImagesPage {
                imagesModel: imagesModel
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
