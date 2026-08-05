pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Shared read-only file browser for Container, Image, and Volume resource refs.
 * The model obtains only the daemon status/path/descriptor over IPC and then
 * performs local FUSE I/O. This component never exposes a write-back action.
 */
Item {
    id: root

    property var filesModel: null
    signal notificationRequested(string message)
    signal volumeRequested(string name)
    signal startServiceRequested()
    signal serviceLogsRequested()

    readonly property string filesState: root.filesModel
                                         ? String(root.filesModel.filesState)
                                         : "idle"
    readonly property bool showRows: root.filesModel
                                     && (root.filesState === "ready"
                                         || (root.filesState === "loading"
                                             && root.filesModel.count > 0))
    readonly property bool showProviderBanner: root.filesModel
                                               && root.filesModel.providerTitle.length > 0
    readonly property bool building: root.filesState === "index_building"
                                     || root.filesState === "snapshot_building"

    function sortLabel(column, label) {
        if (!root.filesModel || root.filesModel.sortColumn !== column)
            return label
        return label + (root.filesModel.sortDescending ? " ↓" : " ↑")
    }

    function openSave(pathToken, name) {
        saveDialog.pathToken = pathToken
        saveDialog.selectedFile = name
        saveDialog.open()
    }

    function stateTitle() {
        switch (root.filesState) {
        case "daemon_offline": return I18n.i18nd("tuxstack", "TuxStack service is not running.")
        case "fuse_offline": return I18n.i18nd("tuxstack", "Docker filesystem is unavailable.")
        case "docker_offline": return I18n.i18nd("tuxstack", "Docker Engine is unavailable.")
        case "provider_unavailable": return I18n.i18nd("tuxstack", "This filesystem provider is unavailable.")
        case "permission_denied": return I18n.i18nd("tuxstack", "Permission denied.")
        case "index_building": return I18n.i18nd("tuxstack", "Image filesystem index is building.")
        case "snapshot_building": return I18n.i18nd("tuxstack", "Container filesystem snapshot is building.")
        default: return I18n.i18nd("tuxstack", "Files could not be loaded.")
        }
    }

    function stateIcon() {
        switch (root.filesState) {
        case "daemon_offline": return "system-run"
        case "fuse_offline": return "drive-harddisk"
        case "docker_offline": return "network-disconnect"
        case "permission_denied": return "lock"
        case "index_building":
        case "snapshot_building": return "view-refresh"
        default: return "dialog-error"
        }
    }

    function providerStatusLabel() {
        if (!root.filesModel)
            return ""
        switch (root.filesModel.providerStatus) {
        case "ready": return I18n.i18nd("tuxstack", "Status: Ready")
        case "index_building": return I18n.i18nd("tuxstack", "Status: Image index building")
        case "snapshot_building": return I18n.i18nd("tuxstack", "Status: Snapshot building")
        case "permission_denied": return I18n.i18nd("tuxstack", "Status: Permission denied")
        case "unavailable": return I18n.i18nd("tuxstack", "Status: Provider unavailable")
        default: return ""
        }
    }

    function providerSourceLabel() {
        if (!root.filesModel || root.filesModel.providerSource.length === 0)
            return ""
        if (root.filesModel.providerKind === "image")
            return I18n.i18nd("tuxstack", "Image filesystem")
        if (root.filesModel.providerKind === "container_snapshot"
                || root.filesModel.providerKind === "container_archive")
            return I18n.i18nd("tuxstack", "Container filesystem")
        return I18n.i18nd("tuxstack", "Filesystem source")
    }

    Keys.onPressed: function(event) {
        if (!root.filesModel || searchField.activeFocus)
            return
        if (event.key === Qt.Key_F5
                || (event.key === Qt.Key_R && event.modifiers & Qt.ControlModifier)) {
            root.filesModel.refresh()
            event.accepted = true
        } else if (event.key === Qt.Key_Backspace && root.filesModel.canGoUp) {
            root.filesModel.goUp()
            event.accepted = true
        } else if (event.key === Qt.Key_Left && event.modifiers & Qt.AltModifier
                   && root.filesModel.canGoBack) {
            root.filesModel.goBack()
            event.accepted = true
        } else if (event.key === Qt.Key_Up && event.modifiers & Qt.AltModifier
                   && root.filesModel.canGoUp) {
            root.filesModel.goUp()
            event.accepted = true
        } else if (event.key === Qt.Key_F && event.modifiers & Qt.ControlModifier) {
            searchField.forceActiveFocus()
            event.accepted = true
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: root.showProviderBanner
            type: root.filesModel && (root.filesModel.providerStatus === "unavailable"
                                      || root.filesModel.providerStatus === "permission_denied")
                  ? Kirigami.MessageType.Warning : Kirigami.MessageType.Information
            text: root.filesModel
                  ? [root.filesModel.providerTitle + " · " + root.filesModel.consistency,
                     root.providerStatusLabel(),
                     root.filesModel.consistencyDetail,
                     root.filesModel.providerStatusDetail,
                     root.providerSourceLabel()]
                    .filter(value => value.length > 0).join("\n")
                  : ""
            showCloseButton: false
            actions: [
                Kirigami.Action {
                    text: root.filesModel ? root.filesModel.refreshActionText : ""
                    icon.name: "view-refresh"
                    visible: root.filesModel && root.filesModel.canRefreshProvider
                    enabled: root.filesModel && !root.building
                    onTriggered: root.filesModel.refreshProvider()
                },
                Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Open in File Manager")
                    icon.name: "system-file-manager"
                    visible: root.filesModel && root.filesModel.rootPath.length > 0
                    onTriggered: root.filesModel.openInFileManager()
                },
                Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Open in Volumes")
                    icon.name: "drive-harddisk"
                    visible: root.filesModel && root.filesModel.namedVolume.length > 0
                    onTriggered: root.filesModel.openInVolumes()
                },
                Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Open Host Folder")
                    icon.name: "folder-home"
                    visible: root.filesModel && root.filesModel.hostFolder.length > 0
                    onTriggered: root.filesModel.openHostFolder()
                }
            ]
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            QQC2.ToolButton {
                icon.name: "go-previous"
                enabled: root.filesModel && root.filesModel.canGoBack
                Accessible.name: I18n.i18nd("tuxstack", "Back")
                onClicked: root.filesModel.goBack()
            }
            QQC2.ToolButton {
                icon.name: "go-up"
                enabled: root.filesModel && root.filesModel.canGoUp
                Accessible.name: I18n.i18nd("tuxstack", "Up")
                onClicked: root.filesModel.goUp()
            }
            QQC2.ToolButton {
                icon.name: "view-refresh"
                enabled: root.filesModel && !root.filesModel.loading
                Accessible.name: I18n.i18nd("tuxstack", "Refresh Folder")
                onClicked: root.filesModel.refresh()
            }

            Flickable {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 1.6
                Layout.minimumWidth: Kirigami.Units.gridUnit * 6
                contentWidth: breadcrumbRow.implicitWidth
                clip: true
                flickableDirection: Flickable.HorizontalFlick
                boundsBehavior: Flickable.StopAtBounds

                RowLayout {
                    id: breadcrumbRow
                    spacing: 0
                    height: parent.height

                    Repeater {
                        model: root.filesModel ? root.filesModel.breadcrumbModel : []
                        delegate: RowLayout {
                            id: breadcrumbDelegate
                            required property var modelData
                            required property int index
                            spacing: 0

                            QQC2.Label {
                                // The root button already displays "/". Add a
                                // separator only between child components so
                                // paths render as /etc/ssl, never as an image
                                // identity followed by a host namespace path.
                                visible: breadcrumbDelegate.index > 1
                                text: "/"
                                color: Kirigami.Theme.disabledTextColor
                            }
                            QQC2.ToolButton {
                                text: String(breadcrumbDelegate.modelData.label || "")
                                flat: true
                                display: QQC2.AbstractButton.TextOnly
                                onClicked: root.filesModel.navigateTo(
                                    String(breadcrumbDelegate.modelData.pathToken || "/"))
                            }
                        }
                    }
                }
            }

            QQC2.TextField {
                id: searchField
                Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                Layout.maximumWidth: Kirigami.Units.gridUnit * 16
                placeholderText: I18n.i18nd("tuxstack", "Search this folder…")
                text: root.filesModel ? root.filesModel.searchQuery : ""
                onTextEdited: root.filesModel && root.filesModel.setSearchQuery(text)
            }
            QQC2.ToolButton {
                icon.name: "view-hidden"
                checkable: true
                checked: root.filesModel ? root.filesModel.showHidden : false
                Accessible.name: I18n.i18nd("tuxstack", "Show Hidden Files")
                onToggled: root.filesModel && root.filesModel.setShowHidden(checked)
            }
        }

        Kirigami.Separator { Layout.fillWidth: true }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ColumnLayout {
                anchors.fill: parent
                spacing: 0
                visible: root.showRows

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 1.8
                    color: Kirigami.Theme.alternateBackgroundColor

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Kirigami.Units.smallSpacing
                        anchors.rightMargin: Kirigami.Units.smallSpacing
                        spacing: Kirigami.Units.smallSpacing

                        QQC2.ToolButton {
                            Layout.fillWidth: true
                            Layout.preferredWidth: 4
                            display: QQC2.AbstractButton.TextOnly
                            text: root.sortLabel("name", I18n.i18nd("tuxstack", "Name"))
                            font.bold: true
                            onClicked: root.filesModel.toggleSort("name")
                        }
                        QQC2.ToolButton {
                            Layout.fillWidth: true
                            Layout.preferredWidth: 2
                            display: QQC2.AbstractButton.TextOnly
                            text: root.sortLabel("modified", I18n.i18nd("tuxstack", "Modified"))
                            font.bold: true
                            onClicked: root.filesModel.toggleSort("modified")
                        }
                        QQC2.ToolButton {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 6
                            display: QQC2.AbstractButton.TextOnly
                            text: root.sortLabel("size", I18n.i18nd("tuxstack", "Size"))
                            font.bold: true
                            onClicked: root.filesModel.toggleSort("size")
                        }
                        QQC2.ToolButton {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 8
                            display: QQC2.AbstractButton.TextOnly
                            text: root.sortLabel("kind", I18n.i18nd("tuxstack", "Kind"))
                            font.bold: true
                            onClicked: root.filesModel.toggleSort("kind")
                        }
                    }
                }

                ListView {
                    id: fileList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: root.filesModel
                    boundsBehavior: Flickable.StopAtBounds
                    QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

                    delegate: QQC2.ItemDelegate {
                        id: rowDelegate
                        required property int index
                        required property string name
                        required property string pathToken
                        required property string displayPath
                        required property string entryType
                        required property string iconName
                        required property string sizeText
                        required property string modifiedText
                        required property string kindText
                        required property bool readable
                        required property bool selected

                        width: ListView.view ? ListView.view.width : 0
                        highlighted: selected
                        hoverEnabled: true

                        contentItem: RowLayout {
                            spacing: Kirigami.Units.smallSpacing
                            Kirigami.Icon {
                                source: rowDelegate.iconName
                                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                Layout.preferredWidth: 4
                                text: rowDelegate.name
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                Layout.preferredWidth: 2
                                text: rowDelegate.modifiedText
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                            }
                            QQC2.Label {
                                Layout.preferredWidth: Kirigami.Units.gridUnit * 6
                                horizontalAlignment: Text.AlignRight
                                text: rowDelegate.sizeText
                                color: Kirigami.Theme.disabledTextColor
                            }
                            QQC2.Label {
                                Layout.preferredWidth: Kirigami.Units.gridUnit * 8
                                text: rowDelegate.kindText
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                            }
                        }

                        onClicked: root.filesModel.selectEntry(rowDelegate.pathToken)
                        onDoubleClicked: root.filesModel.openEntry(rowDelegate.pathToken)

                        QQC2.Menu {
                            id: contextMenu
                            QQC2.MenuItem {
                                text: rowDelegate.entryType === "directory"
                                      ? I18n.i18nd("tuxstack", "Open")
                                      : I18n.i18nd("tuxstack", "Preview")
                                enabled: rowDelegate.readable
                                onTriggered: root.filesModel.openEntry(rowDelegate.pathToken)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Save As…")
                                visible: rowDelegate.entryType === "file"
                                onTriggered: root.openSave(rowDelegate.pathToken, rowDelegate.name)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Properties")
                                onTriggered: root.filesModel.loadProperties(rowDelegate.pathToken)
                            }
                        }

                        TapHandler {
                            acceptedButtons: Qt.RightButton
                            onTapped: {
                                root.filesModel.selectEntry(rowDelegate.pathToken)
                                contextMenu.popup()
                            }
                        }
                    }
                }
            }

            QQC2.BusyIndicator {
                anchors.centerIn: parent
                visible: root.filesModel && root.filesModel.loading && !root.showRows
                running: visible
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                visible: root.filesState === "empty"
                icon.name: "folder"
                text: I18n.i18nd("tuxstack", "This folder is empty.")
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                Kirigami.Units.gridUnit * 28)
                visible: ["daemon_offline", "fuse_offline", "docker_offline",
                          "provider_unavailable", "permission_denied",
                          "index_building", "snapshot_building", "error"].includes(root.filesState)
                icon.name: root.stateIcon()
                text: root.stateTitle()
                explanation: root.filesModel ? root.filesModel.errorMessage : ""
                helpfulAction: Kirigami.Action {
                    text: root.filesState === "daemon_offline"
                          ? I18n.i18nd("tuxstack", "Start Service")
                          : root.filesState === "fuse_offline"
                            ? I18n.i18nd("tuxstack", "Mount")
                            : root.building && root.filesModel
                              ? root.filesModel.refreshActionText
                              : I18n.i18nd("tuxstack", "Retry")
                    icon.name: root.filesState === "daemon_offline" ? "system-run" : "view-refresh"
                    enabled: !root.building
                    onTriggered: {
                        if (root.filesState === "daemon_offline")
                            root.filesModel.requestStartService()
                        else if (root.filesState === "fuse_offline")
                            root.filesModel.mountFilesystem()
                        else if (root.building)
                            root.filesModel.refreshProvider()
                        else
                            root.filesModel.retry()
                    }
                }
            }

            QQC2.Button {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Kirigami.Units.largeSpacing
                visible: root.filesState === "daemon_offline"
                text: I18n.i18nd("tuxstack", "Open Service Logs")
                icon.name: "view-list-text"
                onClicked: root.filesModel.requestServiceLogs()
            }
        }
    }

    LocalFuseFilePreviewDialog {
        id: previewDialog
        filesModel: root.filesModel
        onSaveAsRequested: function(pathToken, name) { root.openSave(pathToken, name) }
    }

    LocalFuseFilePropertiesDialog {
        id: propertiesDialog
        filesModel: root.filesModel
    }

    FileDialog {
        id: saveDialog
        property string pathToken: ""
        title: I18n.i18nd("tuxstack", "Save File As")
        fileMode: FileDialog.SaveFile
        acceptLabel: I18n.i18nd("tuxstack", "Save")
        onAccepted: root.filesModel.saveEntry(pathToken, selectedFile.toString())
    }

    Connections {
        target: root.filesModel
        function onOpenLocalUrl(url) { Qt.openUrlExternally(url) }
        function onVolumeRequested(name) { root.volumeRequested(name) }
        function onStartServiceRequested() { root.startServiceRequested() }
        function onServiceLogsRequested() { root.serviceLogsRequested() }
        function onPreviewReady() { previewDialog.open() }
        function onPreviewFailed(message) { root.notificationRequested(message) }
        function onSaveCompleted(destination) {
            root.notificationRequested(I18n.i18nd("tuxstack", "Saved to %1", destination))
        }
        function onSaveFailed(message) { root.notificationRequested(message) }
        function onPropertiesReady() { propertiesDialog.open() }
        function onNotificationRequested(message) { root.notificationRequested(message) }
    }
}
