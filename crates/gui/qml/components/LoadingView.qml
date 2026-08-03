import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/**
 * Loading indicator with optional message.
 */
Item {
    id: root

    property string message: "Loading…"

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        anchors.verticalCenterOffset: -Kirigami.Units.gridUnit
        running: true
    }

    QQC2.Label {
        anchors.top: parent.verticalCenter
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: Kirigami.Units.largeSpacing
        text: root.message
        color: Kirigami.Theme.disabledTextColor
    }
}
