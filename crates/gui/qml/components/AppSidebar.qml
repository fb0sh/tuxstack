import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Responsive application navigation composed from reusable sidebar sections.
 */
Rectangle {
    id: root

    property string currentPage: "containers"
    property bool collapsed: false
    property bool collapseEnabled: true

    signal pageRequested(string pageId)
    signal collapseRequested()

    readonly property real expandedWidth: Kirigami.Units.gridUnit * 13
    readonly property real collapsedWidth: Kirigami.Units.gridUnit * 3

    readonly property var dockerItems: [
        { pageId: "containers", text: qsTr("Containers"), iconName: "system-run", enabled: true },
        { pageId: "images", text: qsTr("Images"), iconName: "drive-harddisk", enabled: true },
        { pageId: "volumes", text: qsTr("Volumes"), iconName: "folder", enabled: true },
        { pageId: "networks", text: qsTr("Networks"), iconName: "network-wired", enabled: true }
    ]
    readonly property var generalItems: [
        { pageId: "activity", text: qsTr("Activity Monitor"), iconName: "utilities-system-monitor", enabled: true },
        { pageId: "commands", text: qsTr("Commands"), iconName: "utilities-terminal", enabled: true },
        { pageId: "devices", text: qsTr("Devices"), iconName: "drive-removable-media", enabled: true }
    ]
    readonly property var settingsItems: [
        { pageId: "settings", text: qsTr("Settings"), iconName: "configure", enabled: true }
    ]

    implicitWidth: collapsed ? collapsedWidth : expandedWidth
    color: Kirigami.Theme.alternateBackgroundColor

    Behavior on implicitWidth {
        NumberAnimation {
            duration: Kirigami.Units.shortDuration
            easing.type: Easing.InOutQuad
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 3

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.rightMargin: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.mediumSpacing
                visible: !root.collapsed

                Kirigami.Heading {
                    text: qsTr("TuxStack")
                    level: 2
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                }

                SidebarCollapseButton {
                    collapsed: false
                    enabled: root.collapseEnabled
                    onTriggered: root.collapseRequested()
                }
            }

            SidebarCollapseButton {
                anchors.centerIn: parent
                collapsed: true
                enabled: root.collapseEnabled
                visible: root.collapsed && root.collapseEnabled
                onTriggered: root.collapseRequested()
            }
        }

        QQC2.ScrollView {
            id: navigationScroll

            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff
            QQC2.ScrollBar.vertical.policy: QQC2.ScrollBar.AsNeeded

            ColumnLayout {
                width: navigationScroll.availableWidth
                height: Math.max(implicitHeight, navigationScroll.availableHeight)
                spacing: 0

                SidebarSection {
                    Layout.fillWidth: true
                    title: qsTr("Docker")
                    items: root.dockerItems
                    currentPage: root.currentPage
                    collapsed: root.collapsed
                    onPageRequested: function(pageId) {
                        root.pageRequested(pageId)
                    }
                }

                SidebarSection {
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.largeSpacing
                    title: qsTr("General")
                    items: root.generalItems
                    currentPage: root.currentPage
                    collapsed: root.collapsed
                    onPageRequested: function(pageId) {
                        root.pageRequested(pageId)
                    }
                }

                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: Kirigami.Units.largeSpacing
                }

                SidebarSection {
                    Layout.fillWidth: true
                    Layout.bottomMargin: Kirigami.Units.largeSpacing
                    showTitle: false
                    items: root.settingsItems
                    currentPage: root.currentPage
                    collapsed: root.collapsed
                    onPageRequested: function(pageId) {
                        root.pageRequested(pageId)
                    }
                }
            }
        }
    }
}
