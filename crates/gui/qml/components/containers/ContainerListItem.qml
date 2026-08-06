import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.ItemDelegate {
    id: root

    property string containerId: ""
    property string name: ""
    property string image: ""
    property string state: "unknown"
    property string status: ""
    property string health: ""
    property string ports: ""
    property string operation: ""
    property int depth: 0
    property bool selected: false
    property bool logsCapability: false
    property bool terminalCapability: false
    property bool filesCapability: false
    property bool localEndpoint: false
    property var publishedPorts: []
    property var mounts: []

    signal selectedRequested(string id)
    signal startRequested(string id)
    signal stopRequested(string id)
    signal pauseRequested(string id)
    signal unpauseRequested(string id)
    signal removeRequested(string id)
    signal renameRequested(string id)
    signal killRequested(string id)
    signal restartRequested(string id)
    signal logsRequested(string id)
    signal terminalRequested(string id)
    signal inAppTerminalRequested(string id)
    signal filesRequested(string id)
    signal browserRequested(string url)
    signal mountRequested(string type, string source, string destination, string volumeName)
    signal notificationRequested(string message)

    readonly property bool busy: root.operation.length > 0
    readonly property bool running: root.state === "running"
    readonly property bool restarting: root.state === "restarting"
    readonly property bool paused: root.state === "paused"

    width: ListView.view ? ListView.view.width : implicitWidth
    implicitHeight: Kirigami.Units.gridUnit * 4
    hoverEnabled: true
    leftPadding: Kirigami.Units.mediumSpacing + root.depth * Kirigami.Units.gridUnit
    rightPadding: Kirigami.Units.smallSpacing
    enabled: !root.busy
    onClicked: root.selectedRequested(root.containerId)
    TapHandler {
        acceptedButtons: Qt.LeftButton
        onDoubleTapped: {
            if (root.running)
                root.terminalRequested(root.containerId)
            else if (root.paused)
                root.notificationRequested("Resume the container before opening a terminal.")
            else if (root.restarting)
                root.notificationRequested("The container is currently restarting.")
            else
                root.notificationRequested("Start the container before opening a terminal.")
        }
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: {
            if (!root.selected)
                root.selectedRequested(root.containerId)
            contextMenu.popup()
        }
    }

    background: Rectangle {
        radius: Kirigami.Units.smallSpacing
        color: root.selected ? Kirigami.Theme.highlightColor
              : root.hovered ? Qt.alpha(Kirigami.Theme.highlightColor, 0.12)
              : "transparent"
    }

    contentItem: RowLayout {
        spacing: Kirigami.Units.mediumSpacing

        Rectangle {
            Layout.preferredWidth: Kirigami.Units.smallSpacing * 2
            Layout.preferredHeight: width
            radius: width / 2
            color: root.health === "unhealthy" ? Kirigami.Theme.negativeTextColor
                 : root.paused || root.state === "restarting" ? Kirigami.Theme.neutralTextColor
                 : root.running ? Kirigami.Theme.positiveTextColor
                 : Kirigami.Theme.disabledTextColor
        }
        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: width
            source: "container-symbolic"
            selected: root.selected
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            QQC2.Label {
                Layout.fillWidth: true
                text: root.name
                font.bold: root.selected
                color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                elide: Text.ElideRight
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: root.image
                color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: root.status + (root.ports.length > 0 ? " · " + root.ports : "")
                color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
            }
        }
        QQC2.BusyIndicator {
            visible: root.busy
            running: visible
            Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
            Layout.preferredHeight: width
        }
        QQC2.ToolButton {
            visible: !root.busy
            icon.name: root.paused ? "media-playback-start" : root.running ? "media-playback-stop" : "media-playback-start"
            text: root.paused ? I18n.i18nd("tuxstack", "Resume") : root.running ? I18n.i18nd("tuxstack", "Stop") : I18n.i18nd("tuxstack", "Start")
            display: QQC2.AbstractButton.IconOnly
            onClicked: {
                if (root.paused) root.unpauseRequested(root.containerId)
                else if (root.running) root.stopRequested(root.containerId)
                else root.startRequested(root.containerId)
            }
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
        }
        QQC2.ToolButton {
            visible: !root.busy
            icon.name: "edit-delete"
            text: I18n.i18nd("tuxstack", "Delete container")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.removeRequested(root.containerId)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
        }
    }

    ContainerContextMenu {
        id: contextMenu
        containerId: root.containerId
        containerName: root.name
        image: root.image
        state: root.state
        publishedPorts: root.publishedPorts
        mounts: root.mounts
        logsCapability: root.logsCapability
        terminalCapability: root.terminalCapability
        filesCapability: root.filesCapability
        mountNavigationCapability: root.mounts && root.mounts.length > 0
        localEndpoint: root.localEndpoint
        onStartRequested: id => root.startRequested(id)
        onStopRequested: id => root.stopRequested(id)
        onRestartRequested: id => root.restartRequested(id)
        onKillRequested: id => root.killRequested(id)
        onPauseRequested: id => root.pauseRequested(id)
        onUnpauseRequested: id => root.unpauseRequested(id)
        onRemoveRequested: id => root.removeRequested(id)
        onRenameRequested: id => root.renameRequested(id)
        onLogsRequested: id => root.logsRequested(id)
        onTerminalRequested: id => root.terminalRequested(id)
        onInAppTerminalRequested: id => root.inAppTerminalRequested(id)
        onFilesRequested: id => root.filesRequested(id)
        onBrowserRequested: url => root.browserRequested(url)
        onMountRequested: (type, source, destination, volumeName) => root.mountRequested(type, source, destination, volumeName)
        onCopyRequested: function(value) {
            copyField.text = value
            copyField.selectAll()
            copyField.copy()
            copyField.deselect()
        }
    }

    QQC2.TextField {
        id: copyField
        visible: false
    }
}
