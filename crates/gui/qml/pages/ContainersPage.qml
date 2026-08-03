import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Containers page: searchable, filterable container list with actions.
 */
Kirigami.Page {
    id: root

    property var containersModel: null
    property var detailController: null
    property var logModel: null
    property string dockerStatusText: ""

    title: i18nd("tuxstack", "Containers")

    signal openDetailsRequested(string id)

    function refresh() {
        if (containersModel) containersModel.refresh()
    }

    Component.onCompleted: refresh()
    onIsCurrentPageChanged: {
        if (isCurrentPage) refresh()
    }

    actions: [
        Kirigami.Action {
            icon.name: "view-refresh"
            text: i18nd("tuxstack", "Refresh")
            onTriggered: root.refresh()
        }
    ]

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Toolbar
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.topMargin: Kirigami.Units.mediumSpacing
            Layout.bottomMargin: Kirigami.Units.mediumSpacing
            spacing: Kirigami.Units.mediumSpacing

            SearchField {
                Layout.fillWidth: true
                onTextChanged: {
                    if (containersModel) containersModel.searchText = text
                    root.refresh()
                }
            }

            QQC2.ButtonGroup {
                id: stateFilterGroup
                buttons: [allBtn, runningBtn]
            }

            QQC2.ToolButton {
                id: allBtn
                text: i18nd("tuxstack", "All")
                checkable: true
                checked: true
                onToggled: {
                    if (containersModel) containersModel.showAll = checked
                    root.refresh()
                }
            }
            QQC2.ToolButton {
                id: runningBtn
                text: i18nd("tuxstack", "Running")
                checkable: true
                onToggled: {
                    if (containersModel) containersModel.showAll = !checked
                    root.refresh()
                }
            }
        }

        ErrorBanner {
            Layout.fillWidth: true
            textMessage: (containersModel && (containersModel.status === 4 || containersModel.status === 5))
                         ? containersModel.statusText
                         : (dockerStatusText && containersModel && containersModel.status === 5 ? dockerStatusText : "")
        }

        LoadingView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: containersModel && containersModel.status === 1
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: containersModel && containersModel.status === 3
            iconName: "applications-system"
            title: i18nd("tuxstack", "No containers")
            message: i18nd("tuxstack", "No containers match the current filter.")
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: containersModel && containersModel.status === 5
            iconName: "network-offline"
            title: i18nd("tuxstack", "Docker unavailable")
            message: containersModel ? containersModel.statusText : ""
        }

        ListView {
            id: containerList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: containersModel && (containersModel.status === 2 || containersModel.status === 0)
            model: containersModel
            ScrollBar.vertical: QQC2.ScrollBar {}
            spacing: 1

            delegate: Kirigami.AbstractCard {
                width: containerList.width
                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing
                    Layout.margins: Kirigami.Units.mediumSpacing

                    StatusBadge {
                        state: model.state
                        Layout.alignment: Qt.AlignVCenter
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        RowLayout {
                            spacing: Kirigami.Units.smallSpacing
                            QQC2.Label {
                                text: model.name
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                text: model.shortId
                                color: Kirigami.Theme.disabledTextColor
                                font.family: "monospace"
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.BusyIndicator {
                                visible: model.busy
                                running: model.busy
                                implicitWidth: Kirigami.Units.iconSizes.small
                                implicitHeight: Kirigami.Units.iconSizes.small
                            }
                        }

                        RowLayout {
                            spacing: Kirigami.Units.largeSpacing
                            QQC2.Label {
                                text: model.image
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                                Layout.maximumWidth: parent.width * 0.4
                            }
                            QQC2.Label {
                                text: model.status
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                text: model.ports
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                                Layout.maximumWidth: parent.width * 0.35
                            }
                        }

                        RowLayout {
                            spacing: Kirigami.Units.largeSpacing
                            visible: model.running
                            QQC2.Label {
                                text: i18nd("tuxstack", "CPU %1%", model.cpuPercent.toFixed(1))
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: i18nd("tuxstack", "Mem %1 / %2", fmtBytes(model.memoryUsage), fmtBytes(model.memoryLimit))
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: model.createdAt
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                        }
                    }

                    ContainerActions {
                        containerId: model.containerId
                        running: model.running
                        busy: model.busy
                        Layout.alignment: Qt.AlignVCenter
                        onStartRequested: function (id) { containersModel.startContainer(id) }
                        onStopRequested: function (id) { containersModel.stopContainer(id) }
                        onRestartRequested: function (id) { containersModel.restartContainer(id) }
                        onRemoveRequested: function (id) {
                            removeConfirm.message = i18nd("tuxstack",
                                "Remove container “%1”? This cannot be undone.", model.name)
                            removeConfirm.containerId = id
                            removeConfirm.open()
                        }
                    }

                    QQC2.ToolButton {
                        icon.name: "go-next"
                        text: i18nd("tuxstack", "Details")
                        onClicked: root.openDetailsRequested(model.containerId)
                    }
                }
            }
        }
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

    ConfirmRemoveDialog {
        id: removeConfirm
        containerId: ""
        onConfirmed: {
            if (containersModel) containersModel.removeContainer(containerId)
        }
    }
}
