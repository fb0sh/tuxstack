import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var filesModel: null
    title: I18n.i18nd("tuxstack", "Container File Properties")
    preferredWidth: Kirigami.Units.gridUnit * 30

    function propertyObject(value) {
        if (typeof value === "string") {
            try { return JSON.parse(value) } catch (error) { return {} }
        }
        return value || {}
    }

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: root.filesModel ? root.filesModel.properties : []
            delegate: RowLayout {
                id: row
                required property var modelData
                readonly property var item: root.propertyObject(row.modelData)
                Layout.fillWidth: true

                QQC2.Label {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                    text: String(row.item.label || "")
                    color: Kirigami.Theme.disabledTextColor
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    text: String(row.item.value || "")
                    wrapMode: Text.WrapAnywhere
                    font.family: String(row.item.label || "") === "Path" ? "monospace" : font.family
                }
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        standardButtons: QQC2.DialogButtonBox.Close
    }
}
