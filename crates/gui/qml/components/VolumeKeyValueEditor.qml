pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

ColumnLayout {
    id: root

    property string title: ""
    property string addText: I18n.i18nd("tuxstack", "Add entry")
    property string keyPlaceholder: I18n.i18nd("tuxstack", "Key")
    property string valuePlaceholder: I18n.i18nd("tuxstack", "Value")
    property bool editable: true
    readonly property int count: entriesModel.count
    readonly property string validationError: root.validate()

    signal contentChanged()

    spacing: Kirigami.Units.smallSpacing

    function clear() {
        entriesModel.clear()
        root.contentChanged()
    }

    function append(key, value) {
        entriesModel.append({"key": String(key || ""), "value": String(value || "")})
        root.contentChanged()
    }

    function entries() {
        const result = []
        for (let index = 0; index < entriesModel.count; ++index) {
            const row = entriesModel.get(index)
            result.push({"key": String(row.key).trim(), "value": String(row.value)})
        }
        return result
    }

    function validate() {
        const seen = Object.create(null)
        for (let index = 0; index < entriesModel.count; ++index) {
            const key = String(entriesModel.get(index).key).trim()
            if (key.length === 0)
                return I18n.i18nd("tuxstack", "Keys cannot be empty.")
            if (seen[key])
                return I18n.i18nd("tuxstack", "Duplicate key “%1”.", key)
            seen[key] = true
        }
        return ""
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: root.title.length > 0
        text: root.title
        font.bold: true
    }

    Rectangle {
        Layout.fillWidth: true
        visible: entriesModel.count > 0
        implicitHeight: header.implicitHeight + Kirigami.Units.smallSpacing * 2
        color: Kirigami.Theme.alternateBackgroundColor

        RowLayout {
            id: header
            anchors.fill: parent
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                text: I18n.i18nd("tuxstack", "Key")
                font.bold: true
            }
            QQC2.Label {
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                text: I18n.i18nd("tuxstack", "Value")
                font.bold: true
            }
            Item {
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            }
        }
    }

    ListModel { id: entriesModel }

    Repeater {
        model: entriesModel

        delegate: RowLayout {
            id: entryRow
            required property int index
            required property string key
            required property string value

            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.TextField {
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                enabled: root.editable
                text: entryRow.key
                placeholderText: root.keyPlaceholder
                selectByMouse: true
                Accessible.name: I18n.i18nd("tuxstack", "Entry %1 key", entryRow.index + 1)
                onTextEdited: {
                    entriesModel.setProperty(entryRow.index, "key", text)
                    root.contentChanged()
                }
            }
            QQC2.TextField {
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                enabled: root.editable
                text: entryRow.value
                placeholderText: root.valuePlaceholder
                selectByMouse: true
                Accessible.name: I18n.i18nd("tuxstack", "Entry %1 value", entryRow.index + 1)
                onTextEdited: {
                    entriesModel.setProperty(entryRow.index, "value", text)
                    root.contentChanged()
                }
            }
            QQC2.ToolButton {
                icon.name: "list-remove"
                text: I18n.i18nd("tuxstack", "Remove entry %1", entryRow.index + 1)
                display: QQC2.AbstractButton.IconOnly
                enabled: root.editable
                focusPolicy: Qt.StrongFocus
                onClicked: {
                    entriesModel.remove(entryRow.index)
                    root.contentChanged()
                }
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: text
                QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
            }
        }
    }

    QQC2.Button {
        text: root.addText
        icon.name: "list-add"
        enabled: root.editable
        focusPolicy: Qt.StrongFocus
        onClicked: root.append("", "")
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: root.validationError.length > 0
        text: root.validationError
        color: Kirigami.Theme.negativeTextColor
        font: Kirigami.Theme.smallFont
        wrapMode: Text.Wrap
    }
}
