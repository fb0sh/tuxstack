import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/**
 * Compose page: honest planned state. No mock projects.
 */
Kirigami.Page {
    id: root

    title: i18nd("tuxstack", "Compose")

    EmptyState {
        anchors.fill: parent
        iconName: "folder-sync"
        title: i18nd("tuxstack", "Compose — planned")
        message: i18nd("tuxstack",
            "Docker Compose project support is planned.\n\n\
Currently TuxStack manages containers, images, networks and volumes directly. \
Compose projects will be added in a future release — no project data is shown yet.")
    }
}
