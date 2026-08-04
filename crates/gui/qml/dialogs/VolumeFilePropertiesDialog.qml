pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.Dialog {
    id: root

    property var filesModel: null

    title: I18n.i18nd("tuxstack", "Properties")
    modal: true
    standardButtons: QQC2.Dialog.Close
    width: Math.min(Kirigami.Units.gridUnit * 28, parent ? parent.width * 0.9 : Kirigami.Units.gridUnit * 28)
    anchors.centerIn: parent

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: root.filesModel ? root.filesModel.propertiesModel : []
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                QQC2.Label {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 8
                    text: String(modelData.label || "")
                    color: Kirigami.Theme.disabledTextColor
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    text: String(modelData.value || "")
                    wrapMode: Text.WrapAnywhere
                    font.family: String(modelData.label || "") === "Logical Path"
                                 || String(modelData.label || "") === "Permissions"
                                 ? "monospace" : font.family
                }
            }
        }
    }
}
