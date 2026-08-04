import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/** A titled group of sidebar navigation items. */
ColumnLayout {
    id: root

    property string title: ""
    property var items: []
    property string currentPage: ""
    property bool collapsed: false
    property bool showTitle: true

    signal pageRequested(string pageId)

    spacing: Kirigami.Units.smallSpacing

    QQC2.Label {
        text: root.title
        visible: root.showTitle && !root.collapsed
        opacity: visible ? 1 : 0
        color: Kirigami.Theme.disabledTextColor
        font.weight: Font.DemiBold
        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 0.9
        Layout.fillWidth: true
        Layout.leftMargin: Kirigami.Units.largeSpacing
        Layout.rightMargin: Kirigami.Units.largeSpacing
        Layout.bottomMargin: Kirigami.Units.smallSpacing

        Behavior on opacity {
            NumberAnimation { duration: Kirigami.Units.shortDuration }
        }
    }

    Repeater {
        model: root.items

        SidebarItem {
            required property var modelData

            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            text: modelData.text
            iconName: modelData.iconName
            pageId: modelData.pageId
            enabled: modelData.enabled === undefined ? true : modelData.enabled
            selected: root.currentPage === pageId
            collapsed: root.collapsed
            onTriggered: function(pageId) {
                root.pageRequested(pageId)
            }
        }
    }
}
