import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * TuxStack main window: sidebar navigation + Kirigami page stack.
 */
Kirigami.ApplicationWindow {
    id: root
    title: "TuxStack"
    width: 1100
    height: 720
    visible: true
    minimumWidth: 720
    minimumHeight: 480

    // ---- Shared state objects (registered by CXX-Qt) ----
    AppController {
        id: app
        Component.onCompleted: startup()
    }
    ContainerListModel { id: containersModel }
    ImageListModel { id: imagesModel }
    NetworkListModel { id: networksModel }
    VolumeListModel { id: volumesModel }
    ContainerDetailController { id: detailController }
    LogListModel { id: logModel }

    Connections {
        target: app
        function onDockerStatusChanged() {
            if (app.dockerStatus === 1 && pageRow.currentItem) {
                if (pageRow.currentItem.refresh) pageRow.currentItem.refresh()
            }
        }
    }

    // Sidebar + page area (fixed sidebar for a desktop app).
    contentItem: RowLayout {
        spacing: 0

        AppSidebar {
            id: sidebar
            Layout.fillHeight: true
            statusText: root.connectionStatusText
            statusColor: root.connectionStatusColor
            onNavigate: function (pageId) {
                root.navigate(pageId)
            }
        }

        Rectangle {
            Layout.fillHeight: true
            width: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.3
        }

        Kirigami.PageRow {
            id: pageRow
            Layout.fillWidth: true
            Layout.fillHeight: true
            initialPage: overviewPageComponent
        }
    }

    readonly property string connectionStatusText: {
        if (!app) return ""
        if (app.dockerStatus === 1) return i18nd("tuxstack", "Docker connected")
        if (app.dockerStatus === 0) return i18nd("tuxstack", "Connecting…")
        return i18nd("tuxstack", "Docker unavailable")
    }

    readonly property color connectionStatusColor: {
        if (!app || app.dockerStatus === 1) return Kirigami.Theme.positiveTextColor
        if (app.dockerStatus === 0) return Kirigami.Theme.disabledTextColor
        return Kirigami.Theme.negativeTextColor
    }

    // Main pages defined inline; navigation replaces the current page.
    Component {
        id: overviewPageComponent
        OverviewPage {
            appController: app
            engineJson: app ? app.overviewJson : ""
        }
    }
    Component {
        id: containersPageComponent
        ContainersPage {
            containersModel: containersModel
            detailController: detailController
            logModel: logModel
            onOpenDetailsRequested: function (id) {
                root.openContainerDetails(id)
            }
        }
    }
    Component {
        id: imagesPageComponent
        ImagesPage { imagesModel: imagesModel }
    }
    Component {
        id: networksPageComponent
        NetworksPage { networksModel: networksModel }
    }
    Component {
        id: volumesPageComponent
        VolumesPage { volumesModel: volumesModel }
    }
    Component {
        id: composePageComponent
        ComposePage {}
    }
    Component {
        id: settingsPageComponent
        SettingsPage {
            dockerHost: app ? app.dockerHost : ""
        }
    }

    function pageComponent(pageId) {
        switch (pageId) {
        case "overview": return overviewPageComponent
        case "containers": return containersPageComponent
        case "images": return imagesPageComponent
        case "networks": return networksPageComponent
        case "volumes": return volumesPageComponent
        case "compose": return composePageComponent
        case "settings": return settingsPageComponent
        }
        return overviewPageComponent
    }

    function navigate(pageId) {
        pageRow.replace(pageComponent(pageId))
    }

    function openContainerDetails(id) {
        if (detailController) detailController.open(id)
        pageRow.push(containerDetailsPageComponent)
    }

    Component {
        id: containerDetailsPageComponent
        ContainerDetailsPage {
            detailController: detailController
            logModel: logModel
        }
    }
}
