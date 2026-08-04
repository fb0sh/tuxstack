import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Settings")

    EmptyState {
        anchors.fill: parent
        iconName: "configure"
        title: qsTr("Settings")
        message: qsTr("Application settings will be available in a future phase.")
    }
}
