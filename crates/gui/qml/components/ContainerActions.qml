import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/**
 * Action buttons for a container row (start/stop/restart/remove).
 * Buttons are disabled while the container is busy.
 */
Item {
    id: root

    property string containerId: ""
    property bool running: false
    property bool busy: false

    signal startRequested(string id)
    signal stopRequested(string id)
    signal restartRequested(string id)
    signal removeRequested(string id)

    implicitHeight: row.implicitHeight

    Row {
        id: row
        spacing: Kirigami.Units.smallSpacing

        QQC2.ToolButton {
            icon.name: "media-playback-start"
            text: i18nd("tuxstack", "Start")
            visible: !root.running
            enabled: !root.busy
            onClicked: root.startRequested(root.containerId)
        }
        QQC2.ToolButton {
            icon.name: "media-playback-stop"
            text: i18nd("tuxstack", "Stop")
            visible: root.running
            enabled: !root.busy
            onClicked: root.stopRequested(root.containerId)
        }
        QQC2.ToolButton {
            icon.name: "view-refresh"
            text: i18nd("tuxstack", "Restart")
            enabled: !root.busy
            onClicked: root.restartRequested(root.containerId)
        }
        QQC2.ToolButton {
            icon.name: "edit-delete"
            text: i18nd("tuxstack", "Remove")
            enabled: !root.busy
            onClicked: root.removeRequested(root.containerId)
        }
    }
}
