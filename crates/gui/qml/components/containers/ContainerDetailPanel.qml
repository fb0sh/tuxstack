import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var containersModel: null
    property bool logsCapability: false
    property bool terminalCapability: false
    property bool filesCapability: false

    signal retryRequested()
    signal containerRequested(string id)
    signal browserRequested(string url)
    signal volumeRequested(string name)
    signal networkRequested(string id, string name)
    signal hostPathRequested(string path)
    signal projectFolderRequested(string path)

    // This Item always occupies its parent. With no selection every child is
    // invisible, producing the required completely blank third column.
    ContainerInfoView {
        anchors.fill: parent
        visible: root.containersModel
                 && root.containersModel.selectionKind === "container"
                 && root.containersModel.detailState === "ready"
        containersModel: root.containersModel
        onBrowserRequested: function(url) {
            root.containersModel.requestBrowserUrl(url)
        }
        onVolumeRequested: function(name) {
            root.containersModel.requestVolumeNavigation(name)
        }
        onNetworkRequested: function(id, name) {
            root.containersModel.requestNetworkNavigation(id, name)
        }
        onHostPathRequested: function(path) {
            root.containersModel.requestHostPath(path)
        }
    }

    ContainerGroupInfoView {
        anchors.fill: parent
        visible: root.containersModel
                 && root.containersModel.selectionKind === "group"
                 && root.containersModel.detailState === "ready"
        containersModel: root.containersModel
        onContainerRequested: id => root.containerRequested(id)
        onProjectFolderRequested: function(path) {
            root.containersModel.requestHostPath(path)
            root.projectFolderRequested(path)
        }
    }

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        visible: root.containersModel
                 && root.containersModel.selectionKind !== "none"
                 && root.containersModel.detailState === "loading"
        running: visible
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        visible: root.containersModel
                 && root.containersModel.selectionKind !== "none"
                 && root.containersModel.detailState === "error"
        icon.name: "dialog-error"
        text: I18n.i18nd("tuxstack", "Container information unavailable")
        explanation: root.containersModel ? root.containersModel.detailErrorMessage : ""
        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: root.retryRequested()
        }
    }
}
