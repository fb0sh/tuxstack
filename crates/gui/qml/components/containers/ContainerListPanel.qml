pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var containersModel: null
    property bool logsCapability: false
    property bool terminalCapability: false
    property bool filesCapability: false

    signal createRequested()
    signal removeContainerRequested(string id)
    signal renameContainerRequested(string id)
    signal killContainerRequested(string id)
    signal removeGroupRequested(string id)
    signal logsRequested(string id)
    signal terminalRequested(string id)
    signal filesRequested(string id)
    signal browserRequested(string url)
    signal mountRequested(string type, string source, string destination, string volumeName)

    function sortIs(value) {
        return root.containersModel && String(root.containersModel.sortMode) === value
    }
    function chooseSort(value) {
        if (root.containersModel) root.containersModel.setSort(value)
        sortMenu.close()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ColumnLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing
            RowLayout {
                Layout.fillWidth: true
                Kirigami.Heading {
                    Layout.fillWidth: true
                    text: I18n.i18nd("tuxstack", "Containers")
                    level: 2
                }
                QQC2.Label {
                    text: root.containersModel
                          ? I18n.i18nd("tuxstack", "%1 running · %2 total", root.containersModel.runningCount, root.containersModel.totalCount)
                          : I18n.i18nd("tuxstack", "0 running · 0 total")
                    color: Kirigami.Theme.disabledTextColor
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                QQC2.ToolButton {
                    icon.name: "view-sort-ascending"
                    text: I18n.i18nd("tuxstack", "Sort containers")
                    display: QQC2.AbstractButton.IconOnly
                    onClicked: sortMenu.popup()
                    QQC2.Menu {
                        id: sortMenu
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Name A–Z"); checkable: true; checked: root.sortIs("name_asc"); onTriggered: root.chooseSort("name_asc") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Name Z–A"); checkable: true; checked: root.sortIs("name_desc"); onTriggered: root.chooseSort("name_desc") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Newest First"); checkable: true; checked: root.sortIs("newest"); onTriggered: root.chooseSort("newest") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Oldest First"); checkable: true; checked: root.sortIs("oldest"); onTriggered: root.chooseSort("oldest") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Running First"); checkable: true; checked: root.sortIs("running_first"); onTriggered: root.chooseSort("running_first") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Stopped First"); checkable: true; checked: root.sortIs("stopped_first"); onTriggered: root.chooseSort("stopped_first") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Compose Groups First"); checkable: true; checked: root.sortIs("groups_first"); onTriggered: root.chooseSort("groups_first") }
                        QQC2.MenuItem { text: I18n.i18nd("tuxstack", "Individual Containers First"); checkable: true; checked: root.sortIs("individual_first"); onTriggered: root.chooseSort("individual_first") }
                    }
                }
                QQC2.TextField {
                    id: searchField
                    Layout.fillWidth: true
                    placeholderText: I18n.i18nd("tuxstack", "Search containers…")
                    selectByMouse: true
                    onTextChanged: searchDelay.restart()
                }
                QQC2.ToolButton {
                    icon.name: "view-refresh"
                    text: I18n.i18nd("tuxstack", "Refresh containers")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.containersModel && !root.containersModel.refreshing
                    onClicked: root.containersModel.refresh()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                }
                QQC2.ToolButton {
                    icon.name: "list-add"
                    text: I18n.i18nd("tuxstack", "Create container")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.containersModel && !root.containersModel.creating
                    onClicked: root.createRequested()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                }
            }
        }

        Timer {
            id: searchDelay
            interval: 200
            repeat: false
            onTriggered: if (root.containersModel) root.containersModel.setSearch(searchField.text.trim())
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.containersModel && root.containersModel.errorMessage.length > 0
                     && root.containersModel.count > 0
            type: Kirigami.MessageType.Warning
            text: root.containersModel ? root.containersModel.errorMessage : ""
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.containersModel
            visible: root.containersModel
                     && (root.containersModel.listState === "ready"
                         || root.containersModel.listState === "empty"
                         || root.containersModel.count > 0)
            QQC2.ScrollBar.vertical: QQC2.ScrollBar { }

            delegate: Loader {
                id: delegateLoader
                required property var model
                width: list.width
                sourceComponent: String(model.rowKind) === "section" ? sectionDelegate
                               : String(model.rowKind) === "group" ? groupDelegate
                               : containerDelegate
                onLoaded: if (item) item.model = model
            }
        }
    }

    Component {
        id: sectionDelegate
        Rectangle {
            id: sectionRow
            property var model: null
            width: list.width
            height: sectionLabel.implicitHeight + Kirigami.Units.mediumSpacing * 2
            color: Kirigami.Theme.alternateBackgroundColor
            QQC2.Label {
                id: sectionLabel
                anchors.left: parent.left
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.verticalCenter: parent.verticalCenter
                text: String(sectionRow.model.name) + " · " + String(sectionRow.model.status)
                font.bold: true
                color: Kirigami.Theme.disabledTextColor
            }
        }
    }

    Component {
        id: groupDelegate
        ContainerGroupItem {
            property var model: null
            groupId: String(model.id)
            name: String(model.name)
            totalCount: Number(model.groupTotalCount)
            runningCount: Number(model.groupRunningCount)
            pausedCount: Number(model.groupPausedCount)
            stoppedCount: Number(model.groupStoppedCount)
            expanded: Boolean(model.expanded)
            selected: Boolean(model.selected)
            operation: String(model.operation)
            onSelectedRequested: id => root.containersModel.selectRow(id)
            onToggleRequested: id => root.containersModel.toggleGroup(id)
            onStartRequested: id => root.containersModel.startGroup(id)
            onStopRequested: id => root.containersModel.stopGroup(id)
            onRestartRequested: id => root.containersModel.restartGroup(id)
            onPauseRequested: id => root.containersModel.pauseGroup(id)
            onUnpauseRequested: id => root.containersModel.unpauseGroup(id)
            onRemoveRequested: id => root.removeGroupRequested(id)
        }
    }

    Component {
        id: containerDelegate
        ContainerListItem {
            property var model: null
            containerId: String(model.id)
            name: String(model.name)
            image: String(model.image)
            state: String(model.state)
            status: String(model.status)
            health: String(model.health)
            ports: String(model.portsText)
            operation: String(model.operation)
            depth: Number(model.depth)
            selected: Boolean(model.selected)
            logsCapability: root.logsCapability
            terminalCapability: root.terminalCapability
            filesCapability: root.filesCapability
            localEndpoint: root.containersModel ? root.containersModel.localEndpoint : false
            publishedPorts: model.ports || []
            mounts: root.containersModel && root.containersModel.selectionKind === "container"
                    && root.containersModel.selectionId === containerId
                    ? root.containersModel.mountsModel : []
            onSelectedRequested: id => root.containersModel.selectRow(id)
            onStartRequested: id => root.containersModel.startContainer(id)
            onStopRequested: id => root.containersModel.stopContainer(id)
            onPauseRequested: id => root.containersModel.pauseContainer(id)
            onUnpauseRequested: id => root.containersModel.unpauseContainer(id)
            onRestartRequested: id => root.containersModel.restartContainer(id)
            onKillRequested: id => root.killContainerRequested(id)
            onRemoveRequested: id => root.removeContainerRequested(id)
            onRenameRequested: id => root.renameContainerRequested(id)
            onLogsRequested: id => root.logsRequested(id)
            onTerminalRequested: id => root.terminalRequested(id)
            onFilesRequested: id => root.filesRequested(id)
            onBrowserRequested: url => root.containersModel.requestBrowserUrl(url)
            onMountRequested: function(type, source, destination, volumeName) {
                if (type === "volume" && volumeName.length > 0)
                    root.containersModel.requestVolumeNavigation(volumeName)
                else if (type === "bind" && source.length > 0)
                    root.containersModel.requestHostPath(source)
                root.mountRequested(type, source, destination, volumeName)
            }
        }
    }

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        visible: root.containersModel && root.containersModel.loading && root.containersModel.count === 0
        running: visible
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        visible: root.containersModel && !root.containersModel.loading
                 && root.containersModel.count === 0
                 && (root.containersModel.listState === "ready" || root.containersModel.listState === "empty")
        icon.name: "container-symbolic"
        text: searchField.text.trim().length > 0
              ? I18n.i18nd("tuxstack", "No containers match the search.")
              : I18n.i18nd("tuxstack", "No Docker containers found.")
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        visible: root.containersModel && root.containersModel.count === 0
                 && ["error", "docker_unavailable", "permission"].indexOf(root.containersModel.listState) >= 0
        icon.name: root.containersModel && root.containersModel.listState === "permission" ? "dialog-password" : "network-disconnect"
        text: root.containersModel && root.containersModel.listState === "permission"
              ? I18n.i18nd("tuxstack", "TuxStack cannot access Docker.")
              : I18n.i18nd("tuxstack", "Containers could not be loaded.")
        explanation: root.containersModel ? root.containersModel.errorMessage : ""
        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: if (root.containersModel) root.containersModel.refresh()
        }
    }
}
