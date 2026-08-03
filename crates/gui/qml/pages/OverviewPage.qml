import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Overview page: Docker Engine status and resource counts.
 */
Kirigami.Page {
    id: root

    property var appController: null
    property string engineJson: ""

    title: i18nd("tuxstack", "Overview")

    // docker_status: 0 loading, 1 ready, 2 unavailable, 3 permission, 4 error
    readonly property int dockerStatus: appController ? appController.dockerStatus : 0
    readonly property var engine: {
        try {
            return engineJson.length > 0 ? JSON.parse(engineJson) : ({})
        } catch (e) {
            return {}
        }
    }

    actions: [
        Kirigami.Action {
            icon.name: "view-refresh"
            text: i18nd("tuxstack", "Refresh")
            onTriggered: {
                if (appController) appController.refreshOverview()
            }
        }
    ]

    Component.onCompleted: {
        if (appController) appController.refreshOverview()
    }

    onIsCurrentPageChanged: {
        if (isCurrentPage && appController) appController.refreshOverview()
    }

    function fmtBytes(b) {
        const units = ["B", "KB", "MB", "GB", "TB"]
        let value = b
        let unit = 0
        while (value >= 1024 && unit < units.length - 1) {
            value /= 1024
            unit++
        }
        return unit === 0 ? value + " B" : value.toFixed(1) + " " + units[unit]
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.mediumSpacing

        ErrorBanner {
            id: errorBanner
            Layout.fillWidth: true
            textMessage: {
                if (dockerStatus === 0) return ""
                if (dockerStatus === 1) return ""
                return appController ? appController.dockerStatusText : ""
            }
        }

        // Connected summary
        RowLayout {
            Layout.fillWidth: true
            visible: dockerStatus === 1

            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "docker"
                label: i18nd("tuxstack", "Docker Engine")
                value: engine.server_version || "—"
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "settings-configure"
                label: i18nd("tuxstack", "API version")
                value: engine.api_version || "—"
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "computer"
                label: i18nd("tuxstack", "OS / Architecture")
                value: (engine.operating_system || "—") + " (" + (engine.arch || "—") + ")"
            }
        }

        GridLayout {
            Layout.fillWidth: true
            visible: dockerStatus === 1
            columns: Math.max(2, Math.min(4, Math.floor(width / (Kirigami.Units.gridUnit * 8))))
            columnSpacing: Kirigami.Units.mediumSpacing
            rowSpacing: Kirigami.Units.mediumSpacing

            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "applications-system"
                label: i18nd("tuxstack", "Running containers")
                value: String(engine.containers_running ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "applications-system"
                label: i18nd("tuxstack", "Stopped containers")
                value: String(engine.containers_stopped ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "image-x-generic"
                label: i18nd("tuxstack", "Images")
                value: String(engine.images ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "network-server"
                label: i18nd("tuxstack", "Networks")
                value: String(engine.networks ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "drive-harddisk"
                label: i18nd("tuxstack", "Volumes")
                value: String(engine.volumes ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "memory"
                label: i18nd("tuxstack", "Total memory")
                value: fmtBytes(engine.total_memory || 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "cpu"
                label: i18nd("tuxstack", "CPUs")
                value: String(engine.n_cpus ?? 0)
            }
            ResourceSummaryCard {
                Layout.fillWidth: true
                iconName: "folder"
                label: i18nd("tuxstack", "Data root")
                value: engine.docker_root_dir || "—"
            }
        }

        LoadingView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: dockerStatus === 0
            message: i18nd("tuxstack", "Connecting to Docker Engine…")
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: dockerStatus === 2 || dockerStatus === 3
            iconName: dockerStatus === 3 ? "dialog-password" : "network-offline"
            title: dockerStatus === 3
                   ? i18nd("tuxstack", "Permission denied")
                   : i18nd("tuxstack", "Docker Engine unavailable")
            message: appController ? appController.dockerStatusText : ""

            QQC2.Button {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: parent.bottom
                anchors.topMargin: Kirigami.Units.largeSpacing
                text: i18nd("tuxstack", "Try again")
                icon.name: "view-refresh"
                onClicked: {
                    if (appController) appController.startup()
                }
            }
        }

        Item {
            Layout.fillHeight: true
            Layout.fillWidth: true
            visible: dockerStatus === 1
        }
    }
}
