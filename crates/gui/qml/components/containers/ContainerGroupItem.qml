import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.ItemDelegate {
    id: root

    property string groupId: ""
    property string name: ""
    property int totalCount: 0
    property int runningCount: 0
    property int pausedCount: 0
    property int stoppedCount: 0
    property bool expanded: false
    property bool selected: false
    property string operation: ""

    signal selectedRequested(string id)
    signal toggleRequested(string id)
    signal startRequested(string id)
    signal stopRequested(string id)
    signal restartRequested(string id)
    signal pauseRequested(string id)
    signal unpauseRequested(string id)
    signal removeRequested(string id)

    readonly property bool busy: root.operation.length > 0
    width: ListView.view ? ListView.view.width : implicitWidth
    implicitHeight: Kirigami.Units.gridUnit * 4
    hoverEnabled: true
    enabled: !root.busy
    onClicked: root.selectedRequested(root.groupId)
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: {
            if (!root.selected)
                root.selectedRequested(root.groupId)
            groupMenu.popup()
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
        QQC2.ToolButton {
            icon.name: root.expanded ? "arrow-down" : "arrow-right"
            text: root.expanded ? I18n.i18nd("tuxstack", "Collapse group") : I18n.i18nd("tuxstack", "Expand group")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.toggleRequested(root.groupId)
        }
        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: width
            source: "folder-docker"
            selected: root.selected
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            QQC2.Label {
                Layout.fillWidth: true
                text: root.name
                font.bold: true
                color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                elide: Text.ElideRight
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: I18n.i18nd("tuxstack", "%1 / %2 running", root.runningCount, root.totalCount)
                color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
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
            icon.name: root.runningCount > 0 ? "media-playback-stop" : "media-playback-start"
            text: root.runningCount > 0 ? I18n.i18nd("tuxstack", "Stop all") : I18n.i18nd("tuxstack", "Start all")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.runningCount > 0 ? root.stopRequested(root.groupId) : root.startRequested(root.groupId)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
        }
        QQC2.ToolButton {
            visible: !root.busy
            icon.name: "edit-delete"
            text: I18n.i18nd("tuxstack", "Delete group containers")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.removeRequested(root.groupId)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
        }
    }

    QQC2.Menu {
        id: groupMenu
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Start All")
            enabled: root.runningCount < root.totalCount
            onTriggered: root.startRequested(root.groupId)
        }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Stop All")
            enabled: root.runningCount > 0 || root.pausedCount > 0
            onTriggered: root.stopRequested(root.groupId)
        }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Restart All")
            onTriggered: root.restartRequested(root.groupId)
        }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Pause Running")
            enabled: root.runningCount > 0
            onTriggered: root.pauseRequested(root.groupId)
        }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Resume Paused")
            enabled: root.pausedCount > 0
            onTriggered: root.unpauseRequested(root.groupId)
        }
        QQC2.MenuSeparator { }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Copy Project Name")
            icon.name: "edit-copy"
            onTriggered: {
                copyField.text = root.name
                copyField.selectAll()
                copyField.copy()
                copyField.deselect()
            }
        }
        QQC2.MenuItem {
            text: I18n.i18nd("tuxstack", "Delete Containers…")
            icon.name: "edit-delete"
            onTriggered: root.removeRequested(root.groupId)
        }
    }

    QQC2.TextField {
        id: copyField
        visible: false
    }
}
