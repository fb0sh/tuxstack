import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Containers")

    EmptyState {
        anchors.fill: parent
        iconName: "system-run"
        title: qsTr("Containers")
        message: qsTr("Docker container management will be connected in the next phase.")
    }
}
