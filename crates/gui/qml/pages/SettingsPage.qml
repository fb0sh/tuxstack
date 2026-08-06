import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Page {
    id: root

    property var terminalApplicationModel: null

    title: qsTr("Settings")
    padding: Kirigami.Units.largeSpacing

    ColumnLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Kirigami.Units.largeSpacing

        QQC2.Label {
            text: qsTr("Default Terminal")
            font.bold: true
        }

        QQC2.ComboBox {
            id: terminalSelector
            Layout.fillWidth: true
            model: root.terminalApplicationModel
            textRole: "displayName"
            valueRole: "terminalId"
            enabled: root.terminalApplicationModel && root.terminalApplicationModel.count > 0

            Component.onCompleted: {
                if (root.terminalApplicationModel)
                    currentIndex = indexOfValue(root.terminalApplicationModel.selectedTerminalId)
            }
            Connections {
                target: root.terminalApplicationModel
                ignoreUnknownSignals: true
                function onSelectedTerminalIdChanged() {
                    terminalSelector.currentIndex = terminalSelector.indexOfValue(
                                root.terminalApplicationModel.selectedTerminalId)
                }
                function onModelReset() {
                    terminalSelector.currentIndex = terminalSelector.indexOfValue(
                                root.terminalApplicationModel.selectedTerminalId)
                }
            }
            onActivated: {
                if (root.terminalApplicationModel)
                    root.terminalApplicationModel.setDefaultTerminal(currentValue)
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: !root.terminalApplicationModel || root.terminalApplicationModel.count === 0
                  ? qsTr("No supported terminal applications were found. Choose or install a terminal in Settings.")
                  : qsTr("The terminal used when opening a container shell.")
            color: Kirigami.Theme.disabledTextColor
            wrapMode: Text.Wrap
        }

        RowLayout {
            spacing: Kirigami.Units.smallSpacing

            QQC2.Button {
                text: qsTr("Refresh")
                icon.name: "view-refresh"
                onClicked: if (root.terminalApplicationModel)
                                root.terminalApplicationModel.refreshTerminals()
            }
            QQC2.Button {
                text: qsTr("Test Terminal")
                icon.name: "utilities-terminal"
                enabled: terminalSelector.currentIndex >= 0
                onClicked: if (root.terminalApplicationModel)
                                root.terminalApplicationModel.testTerminal(terminalSelector.currentValue)
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            visible: root.terminalApplicationModel
                     && root.terminalApplicationModel.errorMessage.length > 0
            text: root.terminalApplicationModel
                  ? root.terminalApplicationModel.errorMessage : ""
            wrapMode: Text.Wrap
        }
    }
}
