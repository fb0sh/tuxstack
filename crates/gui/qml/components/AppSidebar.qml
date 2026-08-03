import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Left sidebar navigation for TuxStack.
 * Emits `navigate(pageId)` when an entry is selected.
 */
Item {
    id: root

    property alias currentPage: navButtons.currentIndex
    property string statusText: ""
    property color statusColor: "transparent"
    signal navigate(string pageId)

    width: 200
    implicitWidth: 200

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.alternateBackgroundColor
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Kirigami.Heading {
            level: 2
            text: "TuxStack"
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.mediumSpacing
        }

        ListView {
            id: navButtons
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: ListModel {
                ListElement { pageId: "overview"; label: "Overview"; icon: "view-grid-symbolic" }
                ListElement { pageId: "containers"; label: "Containers"; icon: "applications-system" }
                ListElement { pageId: "images"; label: "Images"; icon: "image-x-generic" }
                ListElement { pageId: "networks"; label: "Networks"; icon: "network-server" }
                ListElement { pageId: "volumes"; label: "Volumes"; icon: "drive-harddisk" }
                ListElement { pageId: "compose"; label: "Compose"; icon: "folder-sync" }
                ListElement { pageId: "settings"; label: "Settings"; icon: "settings-configure" }
            }
            delegate: QQC2.ItemDelegate {
                width: ListView.view.width
                height: Kirigami.Units.gridUnit * 2
                highlighted: ListView.isCurrentItem
                onClicked: {
                    navButtons.currentIndex = index
                    root.navigate(model.pageId)
                }
                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing
                    Kirigami.Icon {
                        source: model.icon
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                        color: highlighted ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                    }
                    QQC2.Label {
                        text: model.label
                        color: highlighted ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                        Layout.fillWidth: true
                    }
                }
            }
        }

        Item { Layout.preferredHeight: Kirigami.Units.smallSpacing }

        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.mediumSpacing
            height: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.3
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.mediumSpacing
            text: root.statusText
            color: root.statusColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
            elide: Text.ElideRight
        }
    }
}
