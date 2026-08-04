import QtQuick
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Page {
    title: qsTr("Volumes")

    EmptyState {
        anchors.fill: parent
        iconName: "folder"
        title: qsTr("Volumes")
        message: qsTr("Docker volume management will be connected in the next phase.")
    }
}
