import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/**
 * Empty-state placeholder: icon, title, optional message.
 */
Item {
    id: root

    property string iconName: "dialog-information"
    property string title: "Nothing here yet"
    property string message: ""

    Column {
        anchors.centerIn: parent
        spacing: Kirigami.Units.mediumSpacing
        width: Math.min(parent.width - Kirigami.Units.gridUnit * 4, Kirigami.Units.gridUnit * 14)

        Kirigami.Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            source: root.iconName
            implicitWidth: Kirigami.Units.iconSizes.huge
            implicitHeight: Kirigami.Units.iconSizes.huge
            color: Kirigami.Theme.disabledTextColor
        }

        Kirigami.Heading {
            level: 3
            text: root.title
            horizontalAlignment: Text.AlignHCenter
            anchors.horizontalCenter: parent.horizontalCenter
        }

        QQC2.Label {
            text: root.message
            color: Kirigami.Theme.disabledTextColor
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            anchors.horizontalCenter: parent.horizontalCenter
            visible: text.length > 0
        }
    }
}
