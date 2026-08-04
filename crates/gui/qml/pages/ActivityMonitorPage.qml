import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Activity Monitor")

    EmptyState {
        anchors.fill: parent
        iconName: "utilities-system-monitor"
        title: qsTr("Activity Monitor")
        message: qsTr("Runtime activity monitoring will be connected in a future phase.")
    }
}
