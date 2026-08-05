import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var containersModel: null
    property var statsModel: null
    property var logsModel: null
    property var filesModel: null
    property bool localEndpoint: false
    property bool pageVisible: visible
    property bool statsCapability: statsModel !== null
    property bool logsCapability: logsModel !== null
    property bool terminalCapability: false
    property bool filesCapability: filesModel !== null
    property int currentTab: 0

    signal retryRequested()
    signal containerRequested(string id)
    signal browserRequested(string url)
    signal volumeRequested(string name)
    signal networkRequested(string id, string name)
    signal hostPathRequested(string path)
    signal projectFolderRequested(string path)

    function selectionArrays() {
        const ids = []
        const states = []
        const names = []
        if (!root.containersModel)
            return { "ids": ids, "states": states, "names": names }
        if (root.containersModel.selectionKind === "container") {
            ids.push(String(root.containersModel.selectionId || ""))
            states.push(String(root.containersModel.detailRuntimeState || ""))
            names.push(String(root.containersModel.detailName || root.containersModel.selectionId || ""))
        } else if (root.containersModel.selectionKind === "group") {
            const members = root.containersModel.groupMembersModel || []
            for (let index = 0; index < members.length; ++index) {
                ids.push(String(members[index].id || ""))
                states.push(String(members[index].state || ""))
                names.push(String(members[index].name || members[index].id || ""))
            }
        }
        return { "ids": ids, "states": states, "names": names }
    }

    function syncLiveSelection() {
        const kind = root.containersModel ? String(root.containersModel.selectionKind || "none") : "none"
        const id = root.containersModel ? String(root.containersModel.selectionId || "") : ""
        const values = selectionArrays()
        if (root.statsModel)
            root.statsModel.setSelection(kind, id, values.ids, values.states, values.names)
        if (root.logsModel)
            root.logsModel.setSelection(kind, id, values.ids, values.states, values.names)
        if (root.filesModel) {
            if (kind === "container")
                root.filesModel.selectContainer(id)
            else
                root.filesModel.clearSelection()
        }
    }

    function syncLiveActive() {
        const selected = root.containersModel && root.containersModel.selectionKind !== "none"
        if (root.statsModel)
            root.statsModel.setActive(Boolean(selected && root.pageVisible
                                                 && root.currentTab === 1
                                                 && root.statsCapability))
        if (root.logsModel)
            root.logsModel.setActive(Boolean(selected && root.pageVisible
                                              && root.currentTab === 2
                                              && root.logsCapability))
        if (root.filesModel)
            root.filesModel.setActive(Boolean(root.containersModel
                                               && root.containersModel.selectionKind === "container"
                                               && root.pageVisible
                                               && root.currentTab === 3
                                               && root.filesCapability))
    }

    onCurrentTabChanged: syncLiveActive()
    onPageVisibleChanged: syncLiveActive()
    onStatsModelChanged: { syncLiveSelection(); syncLiveActive() }
    onLogsModelChanged: { syncLiveSelection(); syncLiveActive() }
    onFilesModelChanged: { syncLiveSelection(); syncLiveActive() }
    onStatsCapabilityChanged: syncLiveActive()
    onLogsCapabilityChanged: syncLiveActive()
    Component.onCompleted: { syncLiveSelection(); syncLiveActive() }

    Connections {
        target: root.containersModel
        function onSelectionKindChanged() {
            root.currentTab = 0
            root.syncLiveSelection()
            root.syncLiveActive()
        }
        function onSelectionIdChanged() { root.syncLiveSelection() }
        function onDetailRuntimeStateChanged() { root.syncLiveSelection() }
        function onDetailNameChanged() { root.syncLiveSelection() }
        function onGroupMembersModelChanged() { root.syncLiveSelection() }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.containersModel && root.containersModel.selectionKind !== "none"

        QQC2.TabBar {
            id: tabs
            Layout.fillWidth: true
            currentIndex: root.currentTab
            onCurrentIndexChanged: root.currentTab = currentIndex

            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Info")
            }
            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Stats")
                visible: root.statsCapability
                enabled: root.statsCapability
            }
            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Logs")
                visible: root.logsCapability
                enabled: root.logsCapability
            }
            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Files")
                visible: root.filesCapability
                         && root.containersModel
                         && root.containersModel.selectionKind === "container"
                enabled: visible
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.currentTab

            Item {
                ContainerInfoView {
                    anchors.fill: parent
                    visible: root.containersModel
                             && root.containersModel.selectionKind === "container"
                             && root.containersModel.detailState === "ready"
                    containersModel: root.containersModel
                    onBrowserRequested: function(url) {
                        root.containersModel.requestBrowserUrl(url)
                    }
                    onVolumeRequested: function(name) {
                        root.containersModel.requestVolumeNavigation(name)
                    }
                    onNetworkRequested: function(id, name) {
                        root.containersModel.requestNetworkNavigation(id, name)
                    }
                    onHostPathRequested: function(path) {
                        root.containersModel.requestHostPath(path)
                    }
                }

                ContainerGroupInfoView {
                    anchors.fill: parent
                    visible: root.containersModel
                             && root.containersModel.selectionKind === "group"
                             && root.containersModel.detailState === "ready"
                    containersModel: root.containersModel
                    onContainerRequested: id => root.containerRequested(id)
                    onProjectFolderRequested: function(path) {
                        root.containersModel.requestHostPath(path)
                        root.projectFolderRequested(path)
                    }
                }

                QQC2.BusyIndicator {
                    anchors.centerIn: parent
                    visible: root.containersModel
                             && root.containersModel.selectionKind !== "none"
                             && root.containersModel.detailState === "loading"
                    running: visible
                }

                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    visible: root.containersModel
                             && root.containersModel.selectionKind !== "none"
                             && root.containersModel.detailState === "error"
                    icon.name: "dialog-error"
                    text: I18n.i18nd("tuxstack", "Container information unavailable")
                    explanation: root.containersModel ? root.containersModel.detailErrorMessage : ""
                    helpfulAction: Kirigami.Action {
                        text: I18n.i18nd("tuxstack", "Retry")
                        icon.name: "view-refresh"
                        onTriggered: root.retryRequested()
                    }
                }
            }

            ContainerStatsView {
                statsModel: root.statsModel
            }

            ContainerLogsView {
                logsModel: root.logsModel
            }

            ContainerFilesView {
                filesModel: root.filesModel
                localEndpoint: root.localEndpoint
                onVolumeRequested: name => root.volumeRequested(name)
                onHostPathRequested: path => root.hostPathRequested(path)
            }
        }
    }
}
