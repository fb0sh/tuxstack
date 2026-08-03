import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/**
 * Small colored status pill for a container state.
 */
Rectangle {
    id: root

    property string state: "unknown"
    readonly property bool running: state === "running"
    readonly property bool paused: state === "paused"
    readonly property bool exited: state === "exited"
    readonly property bool created: state === "created"

    readonly property color pillColor: {
        if (state === "running") return Kirigami.Theme.positiveTextColor
        if (state === "paused") return Kirigami.Theme.neutralTextColor
        if (state === "created") return Kirigami.Theme.focusColor
        if (state === "restarting") return Kirigami.Theme.neutralTextColor
        if (state === "dead" || state === "removing") return Kirigami.Theme.negativeTextColor
        return Kirigami.Theme.disabledTextColor // exited / unknown
    }

    radius: height / 2
    color: pillColor
    opacity: 0.15
    implicitWidth: badgeRow.implicitWidth + Kirigami.Units.largeSpacing
    implicitHeight: badgeRow.implicitHeight + Kirigami.Units.smallSpacing

    Row {
        id: badgeRow
        anchors.centerIn: parent
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            width: Kirigami.Units.smallSpacing
            height: width
            radius: width / 2
            anchors.verticalCenter: parent.verticalCenter
            color: root.pillColor
            opacity: 1
        }

        QQC2.Label {
            text: root.state
            color: root.pillColor
            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
            font.bold: true
        }
    }
}
