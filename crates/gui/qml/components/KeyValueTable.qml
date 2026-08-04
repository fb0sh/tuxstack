pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property var sourceModel: null
    property int totalCount: 0
    property string emptyText: qsTr("No entries.")
    property string noMatchesText: qsTr("No matching entries.")
    property string searchPlaceholder: qsTr("Search keys and values…")
    property bool searchable: true
    property bool sortAscending: true

    signal searchRequested(string query)
    signal sortRequested(bool ascending)

    spacing: Kirigami.Units.smallSpacing

    function copyText(item) {
        item.selectAll()
        item.copy()
        item.deselect()
    }

    RowLayout {
        Layout.fillWidth: true
        visible: root.totalCount > 0 && root.searchable
        spacing: Kirigami.Units.smallSpacing

        QQC2.TextField {
            id: searchField
            Layout.fillWidth: true
            placeholderText: root.searchPlaceholder
            selectByMouse: true
            leftPadding: Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing

            Kirigami.Icon {
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.smallSpacing
                anchors.verticalCenter: parent.verticalCenter
                width: Kirigami.Units.iconSizes.smallMedium
                height: width
                source: "edit-find"
                color: Kirigami.Theme.disabledTextColor
            }

            onTextChanged: filterDelay.restart()
        }

        QQC2.ToolButton {
            icon.name: root.sortAscending ? "view-sort-ascending" : "view-sort-descending"
            text: root.sortAscending ? qsTr("Sort descending") : qsTr("Sort ascending")
            display: QQC2.AbstractButton.IconOnly
            onClicked: {
                root.sortAscending = !root.sortAscending
                root.sortRequested(root.sortAscending)
            }
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
        }
    }

    Timer {
        id: filterDelay
        interval: 200
        repeat: false
        onTriggered: root.searchRequested(searchField.text.trim())
    }

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: headerRow.implicitHeight + Kirigami.Units.mediumSpacing
        visible: entryRepeater.count > 0
        color: Kirigami.Theme.alternateBackgroundColor

        RowLayout {
            id: headerRow
            anchors.fill: parent
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.mediumSpacing

            QQC2.Label {
                Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 10, root.width * 0.34)
                text: qsTr("Key")
                font.bold: true
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: qsTr("Value")
                font.bold: true
            }
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            id: entryRepeater
            model: root.sourceModel

            delegate: Item {
                id: tableRow
                required property var model
                Layout.fillWidth: true
                implicitHeight: Math.max(keyText.implicitHeight, valueText.implicitHeight,
                                         copyRow.implicitHeight) + Kirigami.Units.smallSpacing * 2
                property string rowKey: typeof model.key !== "undefined" ? String(model.key) : ""
                property string rowValue: typeof model.value !== "undefined" ? String(model.value) : ""

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.smallSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.mediumSpacing

                    QQC2.TextArea {
                        id: keyText
                        Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 10, root.width * 0.34)
                        text: tableRow.rowKey
                        readOnly: true
                        selectByMouse: true
                        background: null
                        padding: 0
                        font.family: "monospace"
                        wrapMode: TextEdit.WrapAnywhere
                    }

                    QQC2.TextArea {
                        id: valueText
                        Layout.fillWidth: true
                        text: tableRow.rowValue
                        readOnly: true
                        selectByMouse: true
                        background: null
                        padding: 0
                        font.family: "monospace"
                        wrapMode: TextEdit.WrapAnywhere
                    }

                    RowLayout {
                        id: copyRow
                        spacing: 0
                        QQC2.ToolButton {
                            icon.name: "edit-copy"
                            text: qsTr("Copy key")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: root.copyText(keyText)
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                        QQC2.ToolButton {
                            icon.name: "edit-copy"
                            text: qsTr("Copy value")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: root.copyText(valueText)
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                        QQC2.ToolButton {
                            icon.name: "edit-copy"
                            text: qsTr("Copy row")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: {
                                rowCopy.text = tableRow.rowKey + "=" + tableRow.rowValue
                                root.copyText(rowCopy)
                            }
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                    }
                }
            }
        }
    }

    QQC2.TextField {
        id: rowCopy
        visible: false
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: entryRepeater.count === 0
        text: root.totalCount === 0 ? root.emptyText : root.noMatchesText
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }
}
