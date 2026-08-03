import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Live container logs dialog.
 */
Kirigami.Dialog {
    id: root

    property string containerId: ""
    property string containerName: ""
    property var logModel: null
    property var detailController: null
    property bool autoScroll: true

    title: i18nd("tuxstack", "Logs — %1").arg(root.containerName)

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * 36
        Layout.preferredHeight: Kirigami.Units.gridUnit * 24

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            SearchField {
                id: logSearch
                Layout.fillWidth: true
                onTextChanged: {
                    if (root.logModel) {
                        root.logModel.searchText = text
                    }
                }
            }

            QQC2.ToolButton {
                icon.name: "media-playback-start"
                text: i18nd("tuxstack", "Follow")
                checkable: true
                checked: root.detailController ? root.detailController.logsActive : false
                onToggled: {
                    if (!root.detailController) return
                    if (checked) root.detailController.startLogs()
                    else root.detailController.stopLogs()
                }
            }

            QQC2.ToolButton {
                icon.name: "edit-clear"
                text: i18nd("tuxstack", "Clear")
                onClicked: {
                    if (root.logModel) root.logModel.clear()
                }
            }

            QQC2.ToolButton {
                icon.name: "go-down"
                text: i18nd("tuxstack", "Auto-scroll")
                checkable: true
                checked: root.autoScroll
                onToggled: root.autoScroll = checked
            }
        }

        ListView {
            id: logList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.logModel
            ScrollBar.vertical: QQC2.ScrollBar {}
            onCountChanged: {
                if (root.autoScroll) {
                    positionViewAtEnd()
                }
            }
            delegate: RowLayout {
                width: logList.width
                spacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    text: model.timestamp.length > 0 ? model.timestamp : ""
                    color: Kirigami.Theme.disabledTextColor
                    font.family: "monospace"
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    visible: text.length > 0
                }

                QQC2.Label {
                    text: model.message
                    font.family: "monospace"
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    color: model.stream === "stderr"
                           ? Kirigami.Theme.negativeTextColor
                           : Kirigami.Theme.textColor
                    Layout.fillWidth: true
                    wrapMode: Text.WrapAnywhere
                }
            }
        }
    }

    onOpened: {
        if (root.logModel) root.logModel.clear()
        if (root.detailController) root.detailController.startLogs()
    }

    onClosed: {
        if (root.detailController) root.detailController.stopLogs()
    }
}
