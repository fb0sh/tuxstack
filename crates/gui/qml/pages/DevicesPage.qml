import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Devices")

    EmptyState {
        anchors.fill: parent
        iconName: "drive-removable-media"
        title: qsTr("Devices")
        message: qsTr("Device information will be connected in a future phase.")
    }
}
