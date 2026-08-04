import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Container details page: Overview / Logs / Stats / Inspect tabs.
 * Terminal and Files are planned.
 */
Kirigami.Page {
    id: root

    property var detailController: null
    property var logModel: null

    title: detailController && detailController.containerName.length > 0
           ? detailController.containerName
           : I18n.i18nd("tuxstack", "Container details")

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

    onIsCurrentPageChanged: {
        if (!isCurrentPage && detailController) {
            detailController.stopLogs()
            detailController.stopStats()
        }
    }

    Component.onDestruction: {
        if (detailController) {
            detailController.stopLogs()
            detailController.stopStats()
        }
    }

    actions: [
        Kirigami.Action {
            icon.name: "utilities-terminal"
            text: I18n.i18nd("tuxstack", "Logs")
            onTriggered: logsDialog.open()
        },
        Kirigami.Action {
            icon.name: "document-properties"
            text: I18n.i18nd("tuxstack", "Inspect")
            onTriggered: inspectDialog.open()
        },
        Kirigami.Action {
            icon.name: "go-previous"
            text: I18n.i18nd("tuxstack", "Back")
            onTriggered: pageStack.pop()
        }
    ]

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Tab bar
        QQC2.TabBar {
            id: tabBar
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.topMargin: Kirigami.Units.mediumSpacing

            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Overview") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Stats") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Inspect") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Terminal") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Files") }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: tabBar.currentIndex

            // ---- Overview tab ----
            Flickable {
                clip: true
                contentHeight: overviewColumn.implicitHeight
                QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

                ColumnLayout {
                    id: overviewColumn
                    width: parent.width
                    anchors.margins: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.smallSpacing

                    LoadingView {
                        Layout.fillWidth: true
                        Layout.preferredHeight: Kirigami.Units.gridUnit * 6
                        visible: detailController && detailController.detailLoading
                    }

                    QQC2.Label {
                        Layout.fillWidth: true
                        visible: detailController && detailController.detailJson.length > 0
                        text: {
                            if (!detailController) return ""
                            try {
                                const d = JSON.parse(detailController.detailJson)
                                if (d.error) return "Error: " + d.error
                                return ""
                            } catch (e) {
                                return ""
                            }
                        }
                        color: Kirigami.Theme.negativeTextColor
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: {
                            if (!detailController || detailController.detailJson.length === 0) return []
                            try {
                                const d = JSON.parse(detailController.detailJson)
                                if (d.error) return []
                                return [
                                    ["ID", d.summary ? d.summary.id : ""],
                                    ["Image", d.summary ? d.summary.image : ""],
                                    ["State", d.summary ? d.summary.state : ""],
                                    ["Status", d.summary ? d.summary.status : ""],
                                    ["Created", d.summary ? d.summary.created_at : ""],
                                    ["Command", d.command ? d.command.join(" ") : ""],
                                    ["Entrypoint", d.entrypoint ? d.entrypoint.join(" ") : ""],
                                    ["Hostname", d.hostname || "—"],
                                    ["Working dir", d.working_dir || "—"],
                                    ["Platform", d.platform || "—"],
                                    ["Restart policy", d.restart_policy ? d.restart_policy.name : ""],
                                    ["Health", d.health ? d.health.status : "—"],
                                    ["Ports", d.summary && d.summary.ports ? d.summary.ports.map(p => p.host_port ? (p.host_ip || "") + ":" + p.host_port + "->" + p.container_port + "/" + p.protocol : p.container_port + "/" + p.protocol).join(", ") : ""],
                                    ["Mounts", d.mounts ? d.mounts.map(m => (m.source || "") + ":" + m.destination).join(", ") : ""],
                                    ["Networks", d.networks ? d.networks.map(n => n.network_name + (n.ipv4 ? " (" + n.ipv4 + ")" : "")).join(", ") : ""],
                                    ["Memory limit", d.resource_limits && d.resource_limits.memory_bytes ? fmtBytes(d.resource_limits.memory_bytes) : "—"],
                                    ["Nano CPUs", d.resource_limits && d.resource_limits.nano_cpus ? (d.resource_limits.nano_cpus / 1e9).toFixed(2) + " CPUs" : "—"]
                                ]
                            } catch (e) {
                                return []
                            }
                        }
                        delegate: RowLayout {
                            width: overviewColumn.width - Kirigami.Units.largeSpacing * 2
                            spacing: Kirigami.Units.largeSpacing

                            QQC2.Label {
                                text: modelData[0]
                                font.bold: true
                                color: Kirigami.Theme.disabledTextColor
                                Layout.preferredWidth: Kirigami.Units.gridUnit * 8
                            }
                            QQC2.Label {
                                text: modelData[1]
                                Layout.fillWidth: true
                                wrapMode: Text.WrapAnywhere
                                font.family: "monospace"
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                        }
                    }
                }
            }

            // ---- Stats tab ----
            ColumnLayout {
                Layout.margins: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.mediumSpacing

                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Button {
                        text: (detailController && detailController.statsActive)
                              ? I18n.i18nd("tuxstack", "Stop monitoring")
                              : I18n.i18nd("tuxstack", "Start monitoring")
                        icon.name: (detailController && detailController.statsActive) ? "media-playback-stop" : "media-playback-start"
                        onClicked: {
                            if (!detailController) return
                            if (detailController.statsActive) detailController.stopStats()
                            else detailController.startStats()
                        }
                    }
                    QQC2.Label {
                        text: I18n.i18nd("tuxstack", "Sample interval from configuration")
                        color: Kirigami.Theme.disabledTextColor
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: 2
                    columnSpacing: Kirigami.Units.largeSpacing
                    rowSpacing: Kirigami.Units.mediumSpacing

                    QQC2.Label { text: I18n.i18nd("tuxstack", "CPU:") }
                    QQC2.Label { text: detailController ? detailController.cpuPercent.toFixed(1) + " %" : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "Memory:") }
                    QQC2.Label { text: detailController ? detailController.memoryUsage + " / " + detailController.memoryLimit + " (" + detailController.memoryPercent.toFixed(1) + " %)" : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "Network RX:") }
                    QQC2.Label { text: detailController ? detailController.networkRx : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "Network TX:") }
                    QQC2.Label { text: detailController ? detailController.networkTx : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "Block read:") }
                    QQC2.Label { text: detailController ? detailController.blockRead : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "Block write:") }
                    QQC2.Label { text: detailController ? detailController.blockWrite : "—" }

                    QQC2.Label { text: I18n.i18nd("tuxstack", "PIDs:") }
                    QQC2.Label { text: detailController ? detailController.pids : "—" }
                }

                QQC2.Label {
                    text: I18n.i18nd("tuxstack", "CPU history (%)")
                    font.bold: true
                }

                // Simple sparkline from the CSV history property
                Canvas {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 4
                    visible: detailController && detailController.cpuHistory.length > 0

                    property var values: {
                        if (!detailController || detailController.cpuHistory.length === 0) return []
                        try {
                            return detailController.cpuHistory.split(",").map(Number)
                        } catch (e) {
                            return []
                        }
                    }

                    onValuesChanged: requestPaint()
                    onWidthChanged: requestPaint()

                    onPaint: {
                        const ctx = getContext("2d")
                        ctx.reset()
                        const vals = values
                        if (vals.length === 0) return
                        const maxV = Math.max.apply(null, vals.concat([1]))
                        const step = width / vals.length
                        ctx.strokeStyle = Kirigami.Theme.highlightColor
                        ctx.lineWidth = 2
                        ctx.beginPath()
                        for (let i = 0; i < vals.length; i++) {
                            const x = i * step + step / 2
                            const y = height - (height * (vals[i] / maxV))
                            if (i === 0) ctx.moveTo(x, y)
                            else ctx.lineTo(x, y)
                        }
                        ctx.stroke()
                    }
                }
            }

            // ---- Inspect tab ----
            Flickable {
                clip: true
                contentWidth: inspectText.implicitWidth
                contentHeight: inspectText.implicitHeight
                QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
                QQC2.ScrollBar.horizontal: QQC2.ScrollBar {}

                QQC2.TextArea {
                    id: inspectText
                    text: detailController ? detailController.detailJson : ""
                    readOnly: true
                    font.family: "monospace"
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    selectByMouse: true
                    wrapMode: TextEdit.NoWrap
                    background: Rectangle {
                        color: Kirigami.Theme.alternateBackgroundColor
                    }
                }
            }

            // ---- Terminal (planned) ----
            EmptyState {
                iconName: "utilities-terminal"
                title: I18n.i18nd("tuxstack", "Terminal — planned")
                message: I18n.i18nd("tuxstack", "Container terminal support is planned for a future release.")
            }

            // ---- Files (planned) ----
            EmptyState {
                iconName: "folder"
                title: I18n.i18nd("tuxstack", "Files — planned")
                message: I18n.i18nd("tuxstack", "Container file browsing is planned for a future release.")
            }
        }
    }

    ContainerLogsDialog {
        id: logsDialog
        containerName: detailController ? detailController.containerName : ""
        logModel: root.logModel
        detailController: root.detailController
    }

    ContainerInspectDialog {
        id: inspectDialog
        titleText: I18n.i18nd("tuxstack", "Inspect — %1").arg(root.title)
        jsonText: detailController ? detailController.detailJson : ""
    }
}
