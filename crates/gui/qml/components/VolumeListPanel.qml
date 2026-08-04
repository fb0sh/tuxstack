pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var volumesModel: null
    property var keyboardDelegate: null
    readonly property string query: searchField.text.trim()

    signal createRequested()
    signal pruneRequested()
    signal retryRequested()
    signal removeRequested(string volumeName)

    function stateIs(name) {
        return root.volumesModel
               && String(root.volumesModel.listState).toLowerCase() === name
    }

    function sortIs(mode) {
        return root.volumesModel
               && String(root.volumesModel.sortMode) === mode
    }

    function chooseSort(mode) {
        if (root.volumesModel)
            root.volumesModel.setSortMode(mode)
        sortMenu.close()
    }

    function sizeSummary() {
        if (!root.volumesModel)
            return I18n.i18nd("tuxstack", "Volume sizes unavailable")
        const unknown = Number(root.volumesModel.unknownSizeCount)
        const known = Number(root.volumesModel.knownSizeCount)
        const total = String(root.volumesModel.knownTotalSizeText || "")
        if (known <= 0)
            return I18n.i18nd("tuxstack", "Volume sizes unavailable")
        if (unknown > 0) {
            return unknown === 1
                   ? I18n.i18nd("tuxstack", "%1 known · 1 volume unknown", total)
                   : I18n.i18nd("tuxstack", "%1 known · %2 volumes unknown", total, unknown)
        }
        return I18n.i18nd("tuxstack", "%1 total volume data", total)
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
                    text: I18n.i18nd("tuxstack", "Volumes")
                    level: 2
                }

                QQC2.Label {
                    text: {
                        const count = root.volumesModel
                                      ? Number(root.volumesModel.volumeCount) : 0
                        return count === 1
                               ? I18n.i18nd("tuxstack", "1 volume")
                               : I18n.i18nd("tuxstack", "%1 volumes", count)
                    }
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: root.sizeSummary()
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
                QQC2.ToolTip.visible: summaryHover.hovered
                QQC2.ToolTip.text: text
                HoverHandler { id: summaryHover }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.ToolButton {
                    id: sortButton
                    icon.name: "view-sort-ascending"
                    text: I18n.i18nd("tuxstack", "Sort volumes")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.volumesModel !== null
                    focusPolicy: Qt.StrongFocus
                    onClicked: sortMenu.popup()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay

                    QQC2.Menu {
                        id: sortMenu

                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Name A–Z")
                            checkable: true
                            checked: root.sortIs("name_asc")
                            onTriggered: root.chooseSort("name_asc")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Name Z–A")
                            checkable: true
                            checked: root.sortIs("name_desc")
                            onTriggered: root.chooseSort("name_desc")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Newest First")
                            checkable: true
                            checked: root.sortIs("newest")
                            onTriggered: root.chooseSort("newest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Oldest First")
                            checkable: true
                            checked: root.sortIs("oldest")
                            onTriggered: root.chooseSort("oldest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Largest First")
                            checkable: true
                            checked: root.sortIs("largest")
                            onTriggered: root.chooseSort("largest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Smallest First")
                            checkable: true
                            checked: root.sortIs("smallest")
                            onTriggered: root.chooseSort("smallest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Most Containers")
                            checkable: true
                            checked: root.sortIs("most_containers")
                            onTriggered: root.chooseSort("most_containers")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Fewest Containers")
                            checkable: true
                            checked: root.sortIs("fewest_containers")
                            onTriggered: root.chooseSort("fewest_containers")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "In Use First")
                            checkable: true
                            checked: root.sortIs("in_use_first")
                            onTriggered: root.chooseSort("in_use_first")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Unused First")
                            checkable: true
                            checked: root.sortIs("unused_first")
                            onTriggered: root.chooseSort("unused_first")
                        }
                    }
                }

                QQC2.TextField {
                    id: searchField
                    Layout.fillWidth: true
                    placeholderText: I18n.i18nd("tuxstack", "Search volumes…")
                    selectByMouse: true
                    focusPolicy: Qt.StrongFocus
                    leftPadding: Kirigami.Units.iconSizes.smallMedium
                                 + Kirigami.Units.smallSpacing
                    Accessible.name: I18n.i18nd("tuxstack", "Search volumes")

                    Kirigami.Icon {
                        anchors.left: parent.left
                        anchors.leftMargin: Kirigami.Units.smallSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        width: Kirigami.Units.iconSizes.smallMedium
                        height: width
                        source: "edit-find"
                        color: Kirigami.Theme.disabledTextColor
                    }

                    onTextChanged: searchDelay.restart()
                }

                QQC2.ToolButton {
                    icon.name: "view-refresh"
                    text: I18n.i18nd("tuxstack", "Refresh volumes")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.volumesModel && !root.volumesModel.loading
                             && !root.volumesModel.globalOperationInProgress
                    focusPolicy: Qt.StrongFocus
                    onClicked: root.volumesModel.refresh()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                }

                QQC2.ToolButton {
                    icon.name: "list-add"
                    text: I18n.i18nd("tuxstack", "Create volume")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.volumesModel && !root.volumesModel.globalOperationInProgress
                             && (root.stateIs("ready") || root.stateIs("empty"))
                    focusPolicy: Qt.StrongFocus
                    onClicked: root.createRequested()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                }

                QQC2.ToolButton {
                    icon.name: "application-menu"
                    text: I18n.i18nd("tuxstack", "More volume actions")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.volumesModel && !root.volumesModel.globalOperationInProgress
                             && (root.stateIs("ready") || root.stateIs("empty"))
                    focusPolicy: Qt.StrongFocus
                    onClicked: overflowMenu.popup()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay

                    QQC2.Menu {
                        id: overflowMenu
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Remove unused volumes…")
                            icon.name: "edit-clear-history"
                            enabled: root.volumesModel
                                     && Number(root.volumesModel.unusedCount) > 0
                            onTriggered: root.pruneRequested()
                        }
                    }
                }
            }
        }

        Timer {
            id: searchDelay
            interval: 200
            repeat: false
            onTriggered: {
                if (root.volumesModel)
                    root.volumesModel.setSearchQuery(root.query)
            }
        }

        Connections {
            target: root.volumesModel
            ignoreUnknownSignals: true

            function onSearchQueryChanged() {
                const value = root.volumesModel
                              ? String(root.volumesModel.searchQuery) : ""
                if (searchField.text !== value)
                    searchField.text = value
            }
        }

        Kirigami.Separator {
            id: listSeparator
            Layout.fillWidth: true
        }

        ListView {
            id: volumeList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.stateIs("ready") || root.stateIs("empty")
                     || (root.volumesModel && root.volumesModel.loading
                         && root.volumesModel.count > 0)
            activeFocusOnTab: true
            focus: true
            model: root.volumesModel
            currentIndex: -1
            keyNavigationEnabled: true
            section.property: "section"
            section.criteria: ViewSection.FullString
            Accessible.name: I18n.i18nd("tuxstack", "Docker volumes")

            onCurrentIndexChanged: Qt.callLater(function() {
                if (activeFocus && root.keyboardDelegate && root.volumesModel)
                    root.volumesModel.selectVolume(root.keyboardDelegate.volumeName)
            })
            Keys.onPressed: function(event) {
                if (!root.keyboardDelegate)
                    return
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter
                        || event.key === Qt.Key_Space) {
                    if (root.volumesModel)
                        root.volumesModel.selectVolume(root.keyboardDelegate.volumeName)
                    event.accepted = true
                } else if (event.key === Qt.Key_Delete
                           && !root.keyboardDelegate.busy) {
                    root.removeRequested(root.keyboardDelegate.volumeName)
                    event.accepted = true
                }
            }

            section.delegate: Rectangle {
                required property string section
                width: volumeList.width
                height: sectionLabel.implicitHeight + Kirigami.Units.mediumSpacing * 2
                color: Kirigami.Theme.alternateBackgroundColor

                QQC2.Label {
                    id: sectionLabel
                    anchors.left: parent.left
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.verticalCenter: parent.verticalCenter
                    text: parent.section === "in_use"
                          ? I18n.i18nd("tuxstack", "In Use")
                          : I18n.i18nd("tuxstack", "Unused")
                    font.bold: true
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            delegate: Item {
                id: volumeDelegate

                required property int index
                required property string volumeName
                required property string displayName
                required property string driver
                required property string sizeText
                required property bool inUse
                required property int usedByCount
                required property bool anonymous
                required property bool selected
                required property bool busy
                required property string operation

                width: volumeList.width
                implicitHeight: row.implicitHeight

                property bool current: ListView.isCurrentItem
                onCurrentChanged: {
                    if (current)
                        root.keyboardDelegate = volumeDelegate
                    else if (root.keyboardDelegate === volumeDelegate)
                        root.keyboardDelegate = null
                }
                onSelectedChanged: {
                    if (selected && volumeList.currentIndex !== index)
                        volumeList.currentIndex = index
                }
                Component.onCompleted: {
                    if (selected)
                        volumeList.currentIndex = index
                }

                VolumeListItem {
                    id: row
                    width: parent.width
                    volumeName: volumeDelegate.volumeName
                    displayName: volumeDelegate.displayName
                    driver: volumeDelegate.driver
                    sizeText: volumeDelegate.sizeText
                    inUse: volumeDelegate.inUse
                    usedByCount: volumeDelegate.usedByCount
                    anonymous: volumeDelegate.anonymous
                    selected: volumeDelegate.selected
                    busy: volumeDelegate.busy
                    operation: volumeDelegate.operation
                    onSelectedRequested: function(volumeName) {
                        if (root.volumesModel)
                            root.volumesModel.selectVolume(volumeName)
                    }
                    onRemoveRequested: function(volumeName) {
                        root.removeRequested(volumeName)
                    }
                }
            }

            QQC2.ScrollBar.vertical: QQC2.ScrollBar { }
        }
    }

    Column {
        y: listSeparator.mapToItem(root, 0, listSeparator.height).y
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Kirigami.Units.largeSpacing
        visible: root.volumesModel && root.volumesModel.loading
                 && root.volumesModel.count === 0
        spacing: Kirigami.Units.largeSpacing

        Repeater {
            model: 6
            delegate: RowLayout {
                id: skeletonRow
                required property int index
                width: parent ? parent.width : 0
                spacing: Kirigami.Units.mediumSpacing

                Rectangle {
                    Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                    Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                    radius: Kirigami.Units.smallSpacing
                    color: Kirigami.Theme.alternateBackgroundColor
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    Rectangle {
                        Layout.preferredWidth: parent.width
                                               * (skeletonRow.index % 2 === 0 ? 0.58 : 0.72)
                        Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
                        radius: Kirigami.Units.smallSpacing
                        color: Kirigami.Theme.alternateBackgroundColor
                    }
                    Rectangle {
                        Layout.preferredWidth: parent.width * 0.42
                        Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
                        radius: Kirigami.Units.smallSpacing
                        color: Kirigami.Theme.alternateBackgroundColor
                    }
                }
            }
        }
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 20)
        visible: root.volumesModel && !root.volumesModel.loading
                 && volumeList.count === 0
                 && (root.stateIs("ready") || root.stateIs("empty"))
        icon.name: "drive-harddisk"
        text: root.query.length > 0
              ? I18n.i18nd("tuxstack", "No volumes match “%1”.", root.query)
              : I18n.i18nd("tuxstack", "No Docker volumes found.")
        explanation: root.query.length > 0
                     ? I18n.i18nd("tuxstack", "Try a different search term.")
                     : I18n.i18nd("tuxstack", "Create a volume to get started.")
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 20)
        visible: root.stateIs("error") || root.stateIs("dockerunavailable")
                 || root.stateIs("docker_unavailable")
                 || root.stateIs("permissiondenied")
                 || root.stateIs("permission_denied")
        icon.name: root.stateIs("permissiondenied") || root.stateIs("permission_denied")
                   ? "dialog-password" : "network-disconnect"
        text: root.stateIs("permissiondenied") || root.stateIs("permission_denied")
              ? I18n.i18nd("tuxstack", "TuxStack cannot access the Docker socket.")
              : (root.stateIs("dockerunavailable") || root.stateIs("docker_unavailable")
                 ? I18n.i18nd("tuxstack", "Docker Engine is unavailable.")
                 : I18n.i18nd("tuxstack", "Volumes could not be loaded."))
        explanation: root.volumesModel ? String(root.volumesModel.errorMessage) : ""

        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: root.retryRequested()
        }
    }
}
