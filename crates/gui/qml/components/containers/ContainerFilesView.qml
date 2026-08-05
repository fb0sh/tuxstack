pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var filesModel: null
    property bool localEndpoint: false

    signal volumeRequested(string name)
    signal hostPathRequested(string path)
    signal notificationRequested(string message)

    readonly property string filesState: root.filesModel
                                         ? String(root.filesModel.filesState)
                                         : "idle"
    readonly property bool hasSnapshot: root.filesModel
                                        && root.filesModel.snapshotGeneratedAt.length > 0
    readonly property bool showRows: root.filesModel
                                     && (root.filesState === "ready"
                                         || root.filesState === "empty"
                                         || (root.filesState === "loading_directory"
                                             && root.filesModel.count > 0))

    function sortLabel(column, label) {
        if (!root.filesModel || root.filesModel.sortColumn !== column)
            return label
        return label + (root.filesModel.sortDescending ? " ↓" : " ↑")
    }

    function openSave(path, name) {
        saveDialog.containerPath = path
        saveDialog.selectedFile = name
        saveDialog.open()
    }

    Keys.onPressed: function(event) {
        if (!root.filesModel || searchField.activeFocus)
            return
        if (event.key === Qt.Key_F5
                || (event.key === Qt.Key_R && event.modifiers & Qt.ControlModifier)) {
            root.filesModel.refreshSnapshot()
            event.accepted = true
        } else if (event.key === Qt.Key_Backspace && root.filesModel.canGoUp) {
            root.filesModel.goUp()
            event.accepted = true
        } else if (event.key === Qt.Key_Left && event.modifiers & Qt.AltModifier
                   && root.filesModel.canGoBack) {
            root.filesModel.goBack()
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
            visible: root.hasSnapshot
            type: root.filesModel && root.filesModel.snapshotStale
                  ? Kirigami.MessageType.Warning
                  : Kirigami.MessageType.Information
            text: root.filesModel ? root.filesModel.snapshotStatus : ""
            showCloseButton: false
            actions: [
                Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Refresh Snapshot")
                    icon.name: "view-refresh"
                    enabled: root.filesModel && !root.filesModel.refreshingSnapshot
                    onTriggered: root.filesModel.refreshSnapshot()
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

            QQC2.Label {
                Layout.fillWidth: true
                text: root.filesModel ? root.filesModel.currentPath : "/"
                font.family: "monospace"
                elide: Text.ElideMiddle
                QQC2.ToolTip.visible: pathHover.hovered
                QQC2.ToolTip.text: text

                HoverHandler { id: pathHover }
            }

            QQC2.TextField {
                id: searchField
                Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                placeholderText: I18n.i18nd("tuxstack", "Search this folder…")
                text: root.filesModel ? root.filesModel.searchQuery : ""
                onTextEdited: {
                    if (root.filesModel)
                        root.filesModel.setSearchQuery(text)
                }
            }

            QQC2.ToolButton {
                icon.name: "view-hidden"
                checkable: true
                checked: root.filesModel ? root.filesModel.showHidden : false
                Accessible.name: I18n.i18nd("tuxstack", "Show Hidden Files")
                onToggled: {
                    if (root.filesModel)
                        root.filesModel.setShowHidden(checked)
                }
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
                            text: root.sortLabel("type", I18n.i18nd("tuxstack", "Type"))
                            font.bold: true
                            onClicked: root.filesModel.toggleSort("type")
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
                        required property string path
                        required property string entryType
                        required property string iconName
                        required property string sizeText
                        required property string modifiedText
                        required property bool selected
                        required property string origin
                        required property string mountKind
                        required property string mountSource
                        required property string mountDestination
                        required property bool mountReadOnly
                        required property string mountAction

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
                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.preferredWidth: 4
                                spacing: 0
                                QQC2.Label {
                                    Layout.fillWidth: true
                                    text: rowDelegate.name
                                    elide: Text.ElideRight
                                    font.bold: rowDelegate.origin === "mount_overlay"
                                }
                                QQC2.Label {
                                    Layout.fillWidth: true
                                    visible: rowDelegate.origin === "mount_overlay"
                                    text: rowDelegate.mountKind === "volume"
                                          ? I18n.i18nd("tuxstack", "Mounted volume · %1", rowDelegate.mountSource)
                                          : rowDelegate.mountKind === "bind"
                                            ? I18n.i18nd("tuxstack", "Bind mount · %1", rowDelegate.mountSource)
                                            : rowDelegate.mountKind === "tmpfs"
                                              ? I18n.i18nd("tuxstack", "Tmpfs mount · %1",
                                                           rowDelegate.mountReadOnly
                                                           ? I18n.i18nd("tuxstack", "read only")
                                                           : I18n.i18nd("tuxstack", "read/write"))
                                              : I18n.i18nd("tuxstack", "Mounted path")
                                    color: Kirigami.Theme.neutralTextColor
                                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                                    elide: Text.ElideMiddle
                                }
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
                                text: rowDelegate.origin === "mount_overlay"
                                      ? rowDelegate.mountKind
                                      : rowDelegate.entryType
                                color: rowDelegate.origin === "mount_overlay"
                                       ? Kirigami.Theme.neutralTextColor
                                       : Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                            }
                        }

                        onClicked: root.filesModel.selectEntry(rowDelegate.path)
                        onDoubleClicked: root.filesModel.openEntry(rowDelegate.path)

                        QQC2.Menu {
                            id: contextMenu
                            QQC2.MenuItem {
                                text: rowDelegate.mountKind === "volume"
                                      ? I18n.i18nd("tuxstack", "Open Volume Files")
                                      : rowDelegate.mountKind === "bind"
                                        ? I18n.i18nd("tuxstack", "Open Host Folder")
                                        : I18n.i18nd("tuxstack", "Open")
                                enabled: rowDelegate.mountKind !== "bind" || root.localEndpoint
                                visible: rowDelegate.mountKind !== "tmpfs"
                                onTriggered: root.filesModel.openEntry(rowDelegate.path)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Preview")
                                visible: rowDelegate.entryType !== "directory"
                                         && rowDelegate.origin !== "mount_overlay"
                                onTriggered: root.filesModel.previewEntry(rowDelegate.path)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Save As…")
                                visible: rowDelegate.entryType !== "directory"
                                         && rowDelegate.origin !== "mount_overlay"
                                onTriggered: root.openSave(rowDelegate.path, rowDelegate.name)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Properties")
                                onTriggered: root.filesModel.loadProperties(rowDelegate.path)
                            }
                        }

                        TapHandler {
                            acceptedButtons: Qt.RightButton
                            onTapped: {
                                root.filesModel.selectEntry(rowDelegate.path)
                                contextMenu.popup()
                            }
                        }
                    }

                    footer: QQC2.Button {
                        width: fileList.width
                        visible: root.filesModel && root.filesModel.hasMore
                        text: I18n.i18nd("tuxstack", "Load More")
                        enabled: !root.filesModel.loading
                        onClicked: root.filesModel.loadMore()
                    }
                }
            }

            QQC2.BusyIndicator {
                anchors.centerIn: parent
                visible: root.filesModel && root.filesModel.loading
                         && (!root.hasSnapshot || root.filesModel.count === 0)
                running: visible
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                visible: root.filesState === "empty" && root.filesModel
                         && root.filesModel.count === 0
                icon.name: "folder"
                text: I18n.i18nd("tuxstack", "This snapshot folder is empty.")
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                Kirigami.Units.gridUnit * 26)
                visible: root.filesState === "error"
                icon.name: "dialog-error"
                text: I18n.i18nd("tuxstack", "Container filesystem snapshot could not be loaded.")
                explanation: root.filesModel ? root.filesModel.errorMessage : ""
                helpfulAction: Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Refresh Snapshot")
                    icon.name: "view-refresh"
                    onTriggered: root.filesModel.refreshSnapshot()
                }
            }
        }
    }

    ContainerFilePreviewDialog {
        id: previewDialog
        filesModel: root.filesModel
        onSaveAsRequested: function(path) {
            root.openSave(path, root.filesModel ? root.filesModel.previewName : "")
        }
    }

    ContainerFileSaveDialog {
        id: saveDialog
        onSaveRequested: function(path, destination) {
            if (root.filesModel)
                root.filesModel.saveEntry(path, destination)
        }
    }

    ContainerFilePropertiesDialog {
        id: propertiesDialog
        filesModel: root.filesModel
    }

    Timer {
        interval: 1000
        repeat: true
        running: root.visible && root.hasSnapshot
        onTriggered: root.filesModel.updateSnapshotClock()
    }

    Connections {
        target: root.filesModel

        function onVolumeMountRequested(name) { root.volumeRequested(name) }
        function onBindMountRequested(path) {
            if (root.localEndpoint)
                root.hostPathRequested(path)
            else
                root.notificationRequested(I18n.i18nd("tuxstack", "Remote bind source: %1", path))
        }
        function onTmpfsMountRequested(destination, readOnly) {
            root.notificationRequested(I18n.i18nd(
                "tuxstack", "Tmpfs at %1 · %2", destination,
                readOnly ? I18n.i18nd("tuxstack", "read only")
                         : I18n.i18nd("tuxstack", "read/write")))
        }
        function onPreviewReady() { previewDialog.open() }
        function onPreviewFailed(message) { root.notificationRequested(message) }
        function onSaveCompleted(destination) {
            root.notificationRequested(I18n.i18nd("tuxstack", "Saved to %1", destination))
        }
        function onSaveFailed(message) { root.notificationRequested(message) }
        function onPropertiesReady() { propertiesDialog.open() }
    }
}
