import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * TuxStack application shell.
 *
 * The application shell owns the Docker connection and image controller while
 * the fixed StackLayout keeps page navigation responsive and predictable.
 */
Kirigami.ApplicationWindow {
    id: root

    property string currentPage: "containers"
    property string pendingContainerId: ""
    property bool userCollapsed: false

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

    Component.onCompleted: appController.startup()
    onClosing: {
        imagesModel.shutdown()
        networksModel.shutdown()
    }

    Connections {
        target: appController

        function onDockerStatusChanged() {
            imagesModel.setConnectionState(appController.dockerStatus,
                                           appController.dockerStatusText)
            networksModel.setConnectionState(appController.dockerStatus,
                                             appController.dockerStatusText)
        }

        function onDockerStatusTextChanged() {
            if (appController.dockerStatus !== 1) {
                imagesModel.setConnectionState(appController.dockerStatus,
                                               appController.dockerStatusText)
                networksModel.setConnectionState(appController.dockerStatus,
                                                 appController.dockerStatusText)
            }
        }
    }

    Connections {
        target: imagesModel

        function onContainerNavigationRequested(containerId) {
            root.pendingContainerId = containerId
            root.currentPage = "containers"
        }
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
            VolumesPage { }
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
