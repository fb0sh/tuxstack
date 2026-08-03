import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Page header: title, subtitle and a trailing actions area.
 */
Item {
    id: root

    property string title: ""
    property string subtitle: ""
    property alias actions: actionRow.children

    implicitHeight: headerColumn.height + Kirigami.Units.largeSpacing * 2

    ColumnLayout {
        id: headerColumn
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Kirigami.Heading {
            level: 1
            text: root.title
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            QQC2.Label {
                text: root.subtitle
                color: Kirigami.Theme.disabledTextColor
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            Row {
                id: actionRow
                spacing: Kirigami.Units.smallSpacing
                Layout.alignment: Qt.AlignRight
            }
        }
    }
}
