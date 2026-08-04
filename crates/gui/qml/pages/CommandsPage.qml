import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Commands")

    EmptyState {
        anchors.fill: parent
        iconName: "utilities-terminal"
        title: qsTr("Commands")
        message: qsTr("Docker command workflows will be available here in a future phase.")
    }
}
