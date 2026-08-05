pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var filesModel: null
    title: I18n.i18nd("tuxstack", "File Properties")
    preferredWidth: Kirigami.Units.gridUnit * 30
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: root.filesModel ? root.filesModel.propertiesModel : []
            delegate: RowLayout {
                id: row
                required property var modelData
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                QQC2.Label {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                    text: String(row.modelData.label || "")
                    color: Kirigami.Theme.disabledTextColor
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    text: String(row.modelData.value || "")
                    wrapMode: Text.WrapAnywhere
                    font.family: ["Path", "Permissions", "Owner"].includes(
                                     String(row.modelData.label || ""))
                                 ? "monospace" : font.family
                }
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        standardButtons: QQC2.DialogButtonBox.Close
    }
}
