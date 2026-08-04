pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var networksModel: null
    readonly property string query: searchField.text.trim()

    signal createRequested()
    signal retryRequested()
    signal removeRequested(string networkId)

    function stateIs(name) {
        return root.networksModel
               && String(root.networksModel.state).toLowerCase() === name
    }

    function requestRefresh() {
        if (root.networksModel)
            root.networksModel.refresh()
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
                    text: I18n.i18nd("tuxstack", "Networks")
                    level: 2
                }

                QQC2.Label {
                    text: {
                        const value = root.networksModel
                                      ? Number(root.networksModel.totalNetworkCount) : 0
                        const total = isFinite(value) ? value : 0
                        return total === 1
                               ? I18n.i18nd("tuxstack", "1 network")
                               : I18n.i18nd("tuxstack", "%1 networks").arg(total)
                    }
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: root.networksModel
                         && root.networksModel.statusText.length > 0
                         && !root.stateIs("error")
                text: visible ? root.networksModel.statusText : ""
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.ToolButton {
                    id: sortButton
                    icon.name: "view-sort-ascending"
                    text: I18n.i18nd("tuxstack", "Sort networks")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.networksModel !== null
                    onClicked: sortMenu.popup()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text

                    QQC2.Menu {
                        id: sortMenu

                        function choose(mode) {
                            if (root.networksModel)
                                root.networksModel.setSortMode(mode)
                            close()
                        }

                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Name A–Z")
                            checkable: true
                            checked: root.networksModel
                                     && root.networksModel.currentSortMode === "name_asc"
                            onTriggered: sortMenu.choose("name_asc")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Name Z–A")
                            checkable: true
                            checked: root.networksModel
                                     && root.networksModel.currentSortMode === "name_desc"
                            onTriggered: sortMenu.choose("name_desc")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Created newest")
                            checkable: true
                            checked: root.networksModel
                                     && root.networksModel.currentSortMode === "newest"
                            onTriggered: sortMenu.choose("newest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Created oldest")
                            checkable: true
                            checked: root.networksModel
                                     && root.networksModel.currentSortMode === "oldest"
                            onTriggered: sortMenu.choose("oldest")
                        }
                        QQC2.MenuItem {
                            text: I18n.i18nd("tuxstack", "Driver")
                            checkable: true
                            checked: root.networksModel
                                     && root.networksModel.currentSortMode === "driver"
                            onTriggered: sortMenu.choose("driver")
                        }
                    }
                }

                QQC2.TextField {
                    id: searchField
                    Layout.fillWidth: true
                    placeholderText: I18n.i18nd("tuxstack", "Search networks…")
                    selectByMouse: true
                    leftPadding: Kirigami.Units.iconSizes.smallMedium
                                 + Kirigami.Units.smallSpacing

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

                Connections {
                    target: root.networksModel
                    ignoreUnknownSignals: true

                    function onCurrentSearchQueryChanged() {
                        const query = root.networksModel
                                      ? root.networksModel.currentSearchQuery : ""
                        if (searchField.text !== query)
                            searchField.text = query
                    }
                }

                QQC2.ToolButton {
                    icon.name: "list-add"
                    text: I18n.i18nd("tuxstack", "Create network")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.networksModel
                             && !root.networksModel.operationInProgress
                             && (root.stateIs("ready") || root.stateIs("empty"))
                    onClicked: root.createRequested()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                }

                QQC2.ToolButton {
                    icon.name: "view-refresh"
                    text: I18n.i18nd("tuxstack", "Refresh networks")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.networksModel && !root.networksModel.loading
                             && !root.networksModel.operationInProgress
                             && !root.networksModel.removePreparationActive
                    onClicked: root.requestRefresh()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                }
            }
        }

        Timer {
            id: searchDelay
            interval: 200
            repeat: false
            onTriggered: {
                if (root.networksModel)
                    root.networksModel.setSearchQuery(root.query)
            }
        }

        Kirigami.Separator {
            id: listSeparator
            Layout.fillWidth: true
        }

        ListView {
            id: networkList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.stateIs("ready") || root.stateIs("empty")
                     || (root.networksModel && root.networksModel.loading
                         && root.networksModel.count > 0)
            activeFocusOnTab: true
            model: root.networksModel
            currentIndex: -1

            delegate: Item {
                id: networkDelegate

                required property string networkId
                required property string shortId
                required property string name
                required property string subnet
                required property string gateway
                required property string driver
                required property string scope
                required property string createdAt
                required property string createdText
                required property bool internal
                required property bool attachable
                required property bool ingress
                required property bool ipv4
                required property bool ipv6
                required property bool selected
                required property bool busy
                required property string operation

                width: networkList.width
                implicitHeight: row.implicitHeight

                NetworkListItem {
                    id: row
                    width: parent.width
                    networkId: networkDelegate.networkId
                    shortId: networkDelegate.shortId
                    name: networkDelegate.name
                    subnet: networkDelegate.subnet
                    gateway: networkDelegate.gateway
                    driver: networkDelegate.driver
                    scope: networkDelegate.scope
                    internal: networkDelegate.internal
                    attachable: networkDelegate.attachable
                    ingress: networkDelegate.ingress
                    ipv4: networkDelegate.ipv4
                    ipv6: networkDelegate.ipv6
                    selected: networkDelegate.selected
                    busy: networkDelegate.busy
                    operation: networkDelegate.operation

                    onClicked: {
                        if (root.networksModel)
                            root.networksModel.selectNetwork(networkDelegate.networkId)
                    }
                    onRemoveRequested: {
                        root.removeRequested(networkDelegate.networkId)
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
        visible: root.networksModel && root.networksModel.loading
                 && root.networksModel.count === 0
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
        visible: root.networksModel && !root.networksModel.loading
                 && networkList.count === 0
                 && (root.stateIs("ready") || root.stateIs("empty"))
        icon.name: "network-wired"
        text: root.query.length > 0
              ? I18n.i18nd("tuxstack", "No networks match “%1”").arg(root.query)
              : I18n.i18nd("tuxstack", "No Docker networks found.")
        explanation: root.query.length > 0
                     ? I18n.i18nd("tuxstack", "Try a different search term.")
                     : I18n.i18nd("tuxstack", "Create a network to get started.")
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 20)
        visible: root.stateIs("error")
        icon.name: root.networksModel
                   && root.networksModel.errorKind === "permission_denied"
                   ? "dialog-password" : "network-disconnect"
        text: root.networksModel
              && root.networksModel.errorKind === "permission_denied"
              ? I18n.i18nd("tuxstack", "TuxStack cannot access the Docker socket.")
              : (root.networksModel
                 && root.networksModel.errorKind === "docker_unavailable"
                 ? I18n.i18nd("tuxstack", "Docker Engine is unavailable.")
                 : I18n.i18nd("tuxstack", "Networks could not be loaded."))
        explanation: root.networksModel ? root.networksModel.errorMessage : ""

        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: root.retryRequested()
        }
    }
}
