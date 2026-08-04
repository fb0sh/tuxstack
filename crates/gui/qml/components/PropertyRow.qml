import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    property string label: ""
    property string value: ""
    property string copyValue: value
    property string toolTipText: ""
    property bool copyable: false
    property bool monospace: false
    property bool expandable: false
    property bool expanded: false
    property string expandText: qsTr("Show more")
    property string collapseText: qsTr("Show less")
    readonly property bool compact: width < Kirigami.Units.gridUnit * 24

    signal expansionRequested(bool expanded)

    implicitHeight: contentLayout.implicitHeight + Kirigami.Units.smallSpacing * 2

    GridLayout {
        id: contentLayout
        anchors.fill: parent
        anchors.topMargin: Kirigami.Units.smallSpacing
        anchors.bottomMargin: Kirigami.Units.smallSpacing
        columns: root.compact ? 1 : 3
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            Layout.row: 0
            Layout.column: 0
            Layout.preferredWidth: root.compact ? -1 : Kirigami.Units.gridUnit * 10
            Layout.fillWidth: root.compact
            Layout.alignment: Qt.AlignTop
            text: root.label
            color: Kirigami.Theme.disabledTextColor
            wrapMode: root.compact ? Text.Wrap : Text.NoWrap
            elide: root.compact ? Text.ElideNone : Text.ElideRight
        }

        Item {
            Layout.row: 0
            Layout.column: 1
            Layout.fillWidth: !root.compact
            visible: !root.compact
        }

        RowLayout {
            Layout.row: root.compact ? 1 : 0
            Layout.column: root.compact ? 0 : 2
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            spacing: Kirigami.Units.smallSpacing

            QQC2.TextArea {
                id: valueLabel
                Layout.fillWidth: true
                text: root.value.length > 0 ? root.value : qsTr("—")
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.WrapAnywhere
                textFormat: TextEdit.PlainText
                background: null
                padding: 0
                font.family: root.monospace ? "monospace" : Kirigami.Theme.defaultFont.family

                HoverHandler { id: valueHover }
                QQC2.ToolTip.visible: valueHover.hovered
                                          && root.toolTipText.length > 0
                QQC2.ToolTip.text: root.toolTipText
                QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
            }

            QQC2.ToolButton {
                visible: root.expandable
                enabled: visible
                text: root.expanded ? root.collapseText : root.expandText
                icon.name: root.expanded ? "arrow-up" : "arrow-down"
                display: QQC2.AbstractButton.TextBesideIcon
                onClicked: root.expansionRequested(!root.expanded)
            }

            QQC2.ToolButton {
                visible: root.copyable && root.copyValue.length > 0
                enabled: visible
                icon.name: "edit-copy"
                text: qsTr("Copy %1").arg(root.label)
                display: QQC2.AbstractButton.IconOnly
                onClicked: {
                    copySource.selectAll()
                    copySource.copy()
                    copySource.deselect()
                }
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: text
                QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
            }
        }
    }

    QQC2.TextArea {
        id: copySource
        visible: false
        text: root.copyValue
    }
}
