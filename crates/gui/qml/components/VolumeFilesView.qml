pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Read-only volume file browser body.
 *
 * Layout contract:
 *   ColumnLayout
 *     ├── FilesToolbar          (implicit height)
 *     ├── Separator
 *     └── FileTableArea         (fills remaining height)
 *           ├── table (header + rows)
 *           └── centered loading / empty / error overlays
 *
 * Only ONE child of the outer ColumnLayout uses Layout.fillHeight.
 * State overlays are siblings of the table inside that area and use
 * anchors.centerIn — they must never be Layout.fillHeight siblings of
 * the toolbar or they will push the header to the bottom of the pane.
 */
Item {
    id: root

    property var filesModel: null
    property var volumesModel: null

    readonly property string filesState: root.filesModel
                                        ? String(root.filesModel.filesState).toLowerCase()
                                        : "idle"
    readonly property bool showTable: root.filesState === "ready"
                                      || (root.filesState === "loading"
                                          && root.filesModel
                                          && root.filesModel.count > 0)
    readonly property bool showStarting: root.filesState === "starting"
    readonly property bool showLoading: root.filesState === "loading" && !root.showTable
    readonly property bool showEmpty: root.filesState === "empty"
    readonly property bool showError: root.filesState === "error"
    readonly property bool showHelperRequired: root.filesState === "helper_image_required"

    signal notificationRequested(string message)

    function openSelected() {
        if (!root.filesModel || root.filesModel.selectedEntryPath.length === 0)
            return
        root.filesModel.openEntry(root.filesModel.selectedEntryPath)
    }

    function sortLabel(column, label) {
        if (!root.filesModel || root.filesModel.sortColumn !== column)
            return label
        return label + (root.filesModel.sortDescending ? " ↓" : " ↑")
    }

    // Four independent columns. Size and Kind are always separate — never a
    // combined "Size-Kind" cell. Widths are exclusive fixed slots so RowLayout
    // cannot collapse them into each other when space is tight.
    readonly property real tableInnerWidth: Math.max(
        0, (fileArea.width > 0 ? fileArea.width : root.width) - Kirigami.Units.gridUnit)
    readonly property real sizeColWidth: Math.max(Kirigami.Units.gridUnit * 5,
                                                  Math.floor(tableInnerWidth * 0.12))
    readonly property real kindColWidth: Math.max(Kirigami.Units.gridUnit * 6,
                                                  Math.floor(tableInnerWidth * 0.14))
    readonly property real modifiedColWidth: Math.max(Kirigami.Units.gridUnit * 8,
                                                      Math.floor(tableInnerWidth * 0.24))
    readonly property real nameColWidth: Math.max(
        Kirigami.Units.gridUnit * 10,
        tableInnerWidth - modifiedColWidth - sizeColWidth - kindColWidth)

    Keys.onPressed: function(event) {
        if (!root.filesModel)
            return
        if (searchField.activeFocus)
            return
        if (event.key === Qt.Key_Backspace && root.filesModel.canGoUp) {
            root.filesModel.goUp()
            event.accepted = true
        } else if (event.key === Qt.Key_Left && (event.modifiers & Qt.AltModifier)
                   && root.filesModel.canGoBack) {
            root.filesModel.goBack()
            event.accepted = true
        } else if (event.key === Qt.Key_Up && (event.modifiers & Qt.AltModifier)
                   && root.filesModel.canGoUp) {
            root.filesModel.goUp()
            event.accepted = true
        } else if (event.key === Qt.Key_F5
                   || (event.key === Qt.Key_R && (event.modifiers & Qt.ControlModifier))) {
            root.filesModel.refresh()
            event.accepted = true
        } else if (event.key === Qt.Key_F && (event.modifiers & Qt.ControlModifier)) {
            searchField.forceActiveFocus()
            event.accepted = true
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            root.openSelected()
            event.accepted = true
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.filesModel !== null

        // ── Toolbar: Back / Up / Refresh / Breadcrumb / Search / Hidden ──
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: implicitHeight
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
                enabled: root.filesModel
                         && root.filesState !== "starting"
                         && root.filesState !== "idle"
                Accessible.name: I18n.i18nd("tuxstack", "Refresh")
                onClicked: root.filesModel.refresh()
            }

            Flickable {
                id: crumbFlick
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 1.6
                Layout.minimumWidth: Kirigami.Units.gridUnit * 6
                contentWidth: crumbRow.implicitWidth
                clip: true
                flickableDirection: Flickable.HorizontalFlick
                boundsBehavior: Flickable.StopAtBounds

                RowLayout {
                    id: crumbRow
                    spacing: 0
                    height: parent.height

                    Repeater {
                        model: root.filesModel ? root.filesModel.breadcrumbModel : []
                        delegate: RowLayout {
                            id: crumbDelegate
                            required property var modelData
                            required property int index
                            spacing: 0

                            QQC2.Label {
                                visible: crumbDelegate.index > 0
                                text: " / "
                                color: Kirigami.Theme.disabledTextColor
                            }
                            QQC2.ToolButton {
                                text: String(crumbDelegate.modelData.label || "")
                                flat: true
                                display: QQC2.AbstractButton.TextOnly
                                onClicked: {
                                    if (root.filesModel)
                                        root.filesModel.navigateTo(String(crumbDelegate.modelData.path || "/"))
                                }
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

        Kirigami.Separator {
            Layout.fillWidth: true
        }

        // ── Single fill-height content area ──
        Item {
            id: fileArea
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Table (header + rows) fills the area when content is shown.
            ColumnLayout {
                anchors.fill: parent
                spacing: 0
                visible: root.showTable

                Rectangle {
                    id: headerBar
                    Layout.fillWidth: true
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 1.7
                    color: Kirigami.Theme.alternateBackgroundColor
                    clip: true

                    // Plain Row + fixed-width Items: Size and Kind stay distinct columns.
                    Row {
                        id: headerRow
                        anchors.fill: parent
                        anchors.leftMargin: Kirigami.Units.smallSpacing
                        anchors.rightMargin: Kirigami.Units.smallSpacing
                        spacing: 0

                        Item {
                            width: root.nameColWidth
                            height: parent.height
                            QQC2.Label {
                                anchors.fill: parent
                                anchors.rightMargin: Kirigami.Units.smallSpacing
                                verticalAlignment: Text.AlignVCenter
                                text: root.sortLabel("name", I18n.i18nd("tuxstack", "Name"))
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: root.filesModel && root.filesModel.toggleSort("name")
                            }
                        }
                        Item {
                            width: root.modifiedColWidth
                            height: parent.height
                            QQC2.Label {
                                anchors.fill: parent
                                anchors.rightMargin: Kirigami.Units.smallSpacing
                                verticalAlignment: Text.AlignVCenter
                                text: root.sortLabel("modified", I18n.i18nd("tuxstack", "Date Modified"))
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: root.filesModel && root.filesModel.toggleSort("modified")
                            }
                        }
                        Item {
                            width: root.sizeColWidth
                            height: parent.height
                            QQC2.Label {
                                anchors.fill: parent
                                anchors.rightMargin: Kirigami.Units.smallSpacing
                                verticalAlignment: Text.AlignVCenter
                                horizontalAlignment: Text.AlignRight
                                text: root.sortLabel("size", I18n.i18nd("tuxstack", "Size"))
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: root.filesModel && root.filesModel.toggleSort("size")
                            }
                        }
                        Item {
                            width: root.kindColWidth
                            height: parent.height
                            QQC2.Label {
                                anchors.fill: parent
                                verticalAlignment: Text.AlignVCenter
                                text: root.sortLabel("kind", I18n.i18nd("tuxstack", "Kind"))
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: root.filesModel && root.filesModel.toggleSort("kind")
                            }
                        }
                    }
                }

                Kirigami.Separator {
                    Layout.fillWidth: true
                }

                ListView {
                    id: fileList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: root.filesModel
                    currentIndex: -1
                    keyNavigationEnabled: true
                    focus: true
                    boundsBehavior: Flickable.StopAtBounds
                    QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

                    delegate: QQC2.ItemDelegate {
                        id: rowDelegate
                        required property int index
                        required property string name
                        required property string path
                        required property string iconName
                        required property string sizeText
                        required property string modifiedText
                        required property string kindText
                        required property string entryType
                        required property bool selected

                        width: ListView.view ? ListView.view.width : 0
                        height: Kirigami.Units.gridUnit * 1.7
                        highlighted: ListView.isCurrentItem || selected
                        hoverEnabled: true
                        focusPolicy: Qt.StrongFocus
                        // Match header margins exactly so the four columns line up.
                        leftPadding: Kirigami.Units.smallSpacing
                        rightPadding: Kirigami.Units.smallSpacing
                        topPadding: 0
                        bottomPadding: 0

                        contentItem: Row {
                            spacing: 0

                            // Column 0: Name
                            Item {
                                width: root.nameColWidth
                                height: parent.height
                                Row {
                                    anchors.fill: parent
                                    anchors.rightMargin: Kirigami.Units.smallSpacing
                                    spacing: Kirigami.Units.smallSpacing

                                    Kirigami.Icon {
                                        anchors.verticalCenter: parent.verticalCenter
                                        source: rowDelegate.iconName
                                        width: Kirigami.Units.iconSizes.small
                                        height: Kirigami.Units.iconSizes.small
                                    }
                                    QQC2.Label {
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: Math.max(0, parent.width - Kirigami.Units.iconSizes.small
                                                        - Kirigami.Units.smallSpacing)
                                        text: rowDelegate.name
                                        elide: Text.ElideRight
                                    }
                                }
                            }
                            // Column 1: Date Modified
                            Item {
                                width: root.modifiedColWidth
                                height: parent.height
                                QQC2.Label {
                                    anchors.fill: parent
                                    anchors.rightMargin: Kirigami.Units.smallSpacing
                                    verticalAlignment: Text.AlignVCenter
                                    text: rowDelegate.modifiedText
                                    color: Kirigami.Theme.disabledTextColor
                                    elide: Text.ElideRight
                                }
                            }
                            // Column 2: Size (own column — not merged with Kind)
                            Item {
                                width: root.sizeColWidth
                                height: parent.height
                                QQC2.Label {
                                    anchors.fill: parent
                                    anchors.rightMargin: Kirigami.Units.smallSpacing
                                    verticalAlignment: Text.AlignVCenter
                                    horizontalAlignment: Text.AlignRight
                                    text: rowDelegate.sizeText
                                    color: Kirigami.Theme.disabledTextColor
                                    elide: Text.ElideRight
                                }
                            }
                            // Column 3: Kind (own column — not merged with Size)
                            Item {
                                width: root.kindColWidth
                                height: parent.height
                                QQC2.Label {
                                    anchors.fill: parent
                                    verticalAlignment: Text.AlignVCenter
                                    text: rowDelegate.kindText
                                    color: Kirigami.Theme.disabledTextColor
                                    elide: Text.ElideRight
                                }
                            }
                        }

                        onClicked: {
                            fileList.currentIndex = rowDelegate.index
                            if (root.filesModel)
                                root.filesModel.selectEntry(rowDelegate.path)
                        }
                        onDoubleClicked: {
                            if (root.filesModel)
                                root.filesModel.openEntry(rowDelegate.path)
                        }

                        QQC2.Menu {
                            id: contextMenu
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Open")
                                onTriggered: root.filesModel && root.filesModel.openEntry(rowDelegate.path)
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Save As…")
                                visible: rowDelegate.entryType !== "directory"
                                onTriggered: {
                                    downloadDialog.entryPath = rowDelegate.path
                                    downloadDialog.selectedFile = rowDelegate.name
                                    downloadDialog.open()
                                }
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Copy Path")
                                onTriggered: {
                                    root.notificationRequested(
                                        I18n.i18nd("tuxstack", "Path: %1", rowDelegate.path))
                                }
                            }
                            QQC2.MenuItem {
                                text: I18n.i18nd("tuxstack", "Properties")
                                onTriggered: {
                                    if (root.filesModel)
                                        root.filesModel.loadProperties(rowDelegate.path)
                                    propertiesDialog.open()
                                }
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            acceptedButtons: Qt.RightButton
                            onClicked: function(mouse) {
                                if (mouse.button === Qt.RightButton) {
                                    fileList.currentIndex = rowDelegate.index
                                    if (root.filesModel)
                                        root.filesModel.selectEntry(rowDelegate.path)
                                    contextMenu.popup()
                                }
                            }
                        }
                    }
                }
            }

            // Overlays sit on top of the fill area; never Layout.fillHeight
            // siblings of the toolbar.
            LoadingView {
                anchors.centerIn: parent
                width: parent.width
                height: Kirigami.Units.gridUnit * 8
                visible: root.showStarting || root.showLoading
                message: root.showStarting
                         ? I18n.i18nd("tuxstack", "Preparing read-only volume access…")
                         : I18n.i18nd("tuxstack", "Loading folder…")
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                Kirigami.Units.gridUnit * 24)
                visible: root.showEmpty
                icon.name: "folder"
                text: I18n.i18nd("tuxstack", "This folder is empty.")
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                Kirigami.Units.gridUnit * 24)
                visible: root.showError
                icon.name: "dialog-error"
                text: I18n.i18nd("tuxstack", "Volume files could not be loaded.")
                explanation: root.filesModel ? root.filesModel.errorMessage : ""
                helpfulAction: Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Retry")
                    icon.name: "view-refresh"
                    onTriggered: root.filesModel && root.filesModel.retry()
                }
            }

            Kirigami.PlaceholderMessage {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                Kirigami.Units.gridUnit * 24)
                visible: root.showHelperRequired
                icon.name: "download"
                text: I18n.i18nd("tuxstack", "A helper image is required to browse this volume.")
                explanation: root.filesModel
                             ? root.filesModel.errorMessage
                             : I18n.i18nd("tuxstack", "Pull alpine:3.20 to browse volume files.")
                helpfulAction: Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Retry")
                    icon.name: "view-refresh"
                    onTriggered: root.filesModel && root.filesModel.retry()
                }
            }

            QQC2.Label {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: Kirigami.Units.smallSpacing
                visible: root.filesModel && root.filesModel.truncated && root.showTable
                text: I18n.i18nd("tuxstack", "Directory listing was truncated for performance.")
                color: Kirigami.Theme.neutralTextColor
                wrapMode: Text.WordWrap
            }
        }
    }

    VolumeFilePropertiesDialog {
        id: propertiesDialog
        filesModel: root.filesModel
    }

    FileDialog {
        id: downloadDialog
        property string entryPath: ""
        title: I18n.i18nd("tuxstack", "Save Volume File As")
        fileMode: FileDialog.SaveFile
        acceptLabel: I18n.i18nd("tuxstack", "Save")
        onAccepted: {
            if (root.filesModel && entryPath.length > 0)
                root.filesModel.downloadEntry(entryPath, selectedFile.toString())
        }
    }

    Connections {
        target: root.filesModel
        // Double-click / Open uses the host default app; surface launch failures only.
        function onPreviewFailed(message) {
            root.notificationRequested(message)
        }
        function onDownloadCompleted(destinationPath) {
            root.notificationRequested(I18n.i18nd("tuxstack", "Saved to %1", destinationPath))
        }
        function onDownloadFailed(message) {
            root.notificationRequested(message)
        }
        function onSymlinkBlocked(message) {
            root.notificationRequested(message)
        }
    }
}
