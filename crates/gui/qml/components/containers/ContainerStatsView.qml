import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var statsModel: null

    function bytes(value) {
        let number = Math.max(0, Number(value || 0))
        const units = ["B", "KiB", "MiB", "GiB", "TiB"]
        let unit = 0
        while (number >= 1024 && unit < units.length - 1) {
            number /= 1024
            unit++
        }
        return (unit === 0 ? number.toFixed(0) : number.toFixed(1)) + " " + units[unit]
    }

    function metric(label, value, explanation) {
        return { "label": label, "value": value, "explanation": explanation || "" }
    }

    readonly property var metricRows: root.statsModel ? [
        metric(I18n.i18nd("tuxstack", "CPU"), Number(root.statsModel.cpuPercent).toFixed(1) + "%",
               I18n.i18nd("tuxstack", "Sum across container logical CPUs; group totals can exceed 100%.")),
        metric(I18n.i18nd("tuxstack", "Memory raw usage"), root.bytes(root.statsModel.memoryRawBytes),
               I18n.i18nd("tuxstack", "Docker memory_stats.usage.")),
        metric(I18n.i18nd("tuxstack", "Memory working set"),
               root.statsModel.memoryWorkingSetKnown ? root.bytes(root.statsModel.memoryWorkingSetBytes)
                                                     : I18n.i18nd("tuxstack", "Unavailable"),
               I18n.i18nd("tuxstack", "Not substituted with raw usage when the daemon does not expose cache counters.")),
        metric(I18n.i18nd("tuxstack", "Memory limit"), root.bytes(root.statsModel.memoryLimitBytes)),
        metric(I18n.i18nd("tuxstack", "Memory raw %"), Number(root.statsModel.memoryPercent).toFixed(1) + "%"),
        metric(I18n.i18nd("tuxstack", "Network RX / TX"), root.bytes(root.statsModel.networkRxBytes) + " / " + root.bytes(root.statsModel.networkTxBytes)),
        metric(I18n.i18nd("tuxstack", "Block read / write"), root.bytes(root.statsModel.blockReadBytes) + " / " + root.bytes(root.statsModel.blockWriteBytes)),
        metric(I18n.i18nd("tuxstack", "PIDs"), String(root.statsModel.pids))
    ] : []

    Connections {
        target: root.statsModel
        function onHistoryModelChanged() { chart.requestPaint() }
    }

    Flickable {
        anchors.fill: parent
        contentWidth: width
        contentHeight: content.implicitHeight + Kirigami.Units.largeSpacing * 2
        clip: true

        ColumnLayout {
            id: content
            width: parent.width
            spacing: Kirigami.Units.largeSpacing

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: root.statsModel && root.statsModel.status === "error"
                type: Kirigami.MessageType.Error
                text: root.statsModel ? root.statsModel.errorMessage : ""
            }

            RowLayout {
                Layout.fillWidth: true
                QQC2.Label {
                    text: root.statsModel
                          ? I18n.i18nd("tuxstack", "%1 of %2 running containers reporting",
                                             root.statsModel.reportingCount,
                                             root.statsModel.runningCount)
                          : ""
                    color: Kirigami.Theme.disabledTextColor
                }
                Item { Layout.fillWidth: true }
                QQC2.BusyIndicator {
                    visible: root.statsModel && root.statsModel.status === "streaming"
                    running: visible
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: implicitWidth
                }
            }

            GridLayout {
                Layout.fillWidth: true
                columns: width >= Kirigami.Units.gridUnit * 44 ? 4 : 2
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: Kirigami.Units.largeSpacing

                Repeater {
                    model: root.metricRows
                    delegate: Rectangle {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.preferredHeight: Kirigami.Units.gridUnit * 5
                        radius: Kirigami.Units.cornerRadius
                        color: Kirigami.Theme.backgroundColor
                        border.color: Kirigami.Theme.disabledTextColor
                        border.width: 1

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: Kirigami.Units.largeSpacing
                            spacing: Kirigami.Units.smallSpacing
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: String(modelData.label)
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                text: String(modelData.value)
                                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.2
                                font.weight: Font.DemiBold
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                visible: String(modelData.explanation).length > 0
                                text: String(modelData.explanation)
                                color: Kirigami.Theme.disabledTextColor
                                font: Kirigami.Theme.smallFont
                                elide: Text.ElideRight
                                QQC2.ToolTip.visible: metricHover.hovered
                                QQC2.ToolTip.text: text
                                HoverHandler { id: metricHover }
                            }
                        }
                    }
                }
            }

            Kirigami.Heading {
                Layout.fillWidth: true
                level: 3
                text: I18n.i18nd("tuxstack", "History")
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 12
                radius: Kirigami.Units.cornerRadius
                color: Kirigami.Theme.backgroundColor
                border.color: Kirigami.Theme.disabledTextColor
                border.width: 1

                Canvas {
                    id: chart
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.largeSpacing
                    renderTarget: Canvas.Image
                    onWidthChanged: requestPaint()
                    onHeightChanged: requestPaint()
                    onPaint: {
                        const ctx = getContext("2d")
                        ctx.reset()
                        const points = root.statsModel ? root.statsModel.historyModel : []
                        const grid = Kirigami.Theme.disabledTextColor
                        const cpu = Kirigami.Theme.highlightColor
                        const memory = Kirigami.Theme.positiveTextColor
                        ctx.strokeStyle = grid
                        ctx.globalAlpha = 0.25
                        ctx.lineWidth = 1
                        for (let row = 1; row < 4; ++row) {
                            const y = height * row / 4
                            ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(width, y); ctx.stroke()
                        }
                        ctx.globalAlpha = 1
                        if (!points || points.length < 2)
                            return
                        let maxCpu = 100
                        let maxMemory = 1
                        for (let i = 0; i < points.length; ++i) {
                            maxCpu = Math.max(maxCpu, Number(points[i].cpuPercent || 0))
                            maxMemory = Math.max(maxMemory, Number(points[i].memoryRawBytes || 0))
                        }
                        function pathFor(key, maximum, color) {
                            ctx.strokeStyle = color
                            ctx.lineWidth = 2
                            ctx.beginPath()
                            for (let i = 0; i < points.length; ++i) {
                                const x = i * width / Math.max(1, points.length - 1)
                                const y = height - Math.min(maximum, Number(points[i][key] || 0)) * height / maximum
                                if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y)
                            }
                            ctx.stroke()
                        }
                        pathFor("cpuPercent", maxCpu, cpu)
                        pathFor("memoryRawBytes", maxMemory, memory)
                    }
                }
            }

            RowLayout {
                QQC2.Label { text: "●"; color: Kirigami.Theme.highlightColor }
                QQC2.Label { text: I18n.i18nd("tuxstack", "CPU %") }
                Item { Layout.preferredWidth: Kirigami.Units.largeSpacing }
                QQC2.Label { text: "●"; color: Kirigami.Theme.positiveTextColor }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Memory raw usage") }
            }

            Kirigami.Heading {
                Layout.fillWidth: true
                visible: root.statsModel && root.statsModel.runningCount > 1
                level: 3
                text: I18n.i18nd("tuxstack", "Per-container")
            }

            Repeater {
                model: root.statsModel
                delegate: RowLayout {
                    required property string containerName
                    required property real cpuPercent
                    required property real memoryRawBytes
                    required property real networkRxBytes
                    required property real networkTxBytes
                    Layout.fillWidth: true
                    visible: root.statsModel && root.statsModel.runningCount > 1
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 10; text: containerName; elide: Text.ElideRight }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; text: cpuPercent.toFixed(1) + "%" }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 8; text: root.bytes(memoryRawBytes) }
                    QQC2.Label { Layout.fillWidth: true; text: root.bytes(networkRxBytes) + " / " + root.bytes(networkTxBytes); elide: Text.ElideRight }
                }
            }
        }
    }
}
