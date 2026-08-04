pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    property var imagesModel: null
    readonly property string query: searchField.text.trim()

    signal pullRequested()
    signal retryRequested()
    signal removeRequested(string imageId, string displayName, string shortId,
                           string tagsText, string sizeText, int usedByCount)

    function requestRemoval(imageId, displayName, shortId, tagsText, sizeText, usedByCount) {
        root.removeRequested(imageId, displayName, shortId, tagsText, sizeText, usedByCount)
    }

    function stateIs(name) {
        return root.imagesModel && String(root.imagesModel.state).toLowerCase() === name
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
                    text: qsTr("Images")
                    level: 2
                }

                QQC2.Label {
                    text: {
                        const count = root.imagesModel
                                      ? Number(root.imagesModel.totalImageCount) : 0
                        return count === 1 ? qsTr("1 image")
                                           : qsTr("%1 images").arg(count)
                    }
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: root.imagesModel && root.imagesModel.totalSizeText
                      ? qsTr("%1 total image size").arg(root.imagesModel.totalSizeText)
                      : qsTr("0 B total image size")
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.ToolButton {
                    id: sortButton
                    icon.name: "view-sort-ascending"
                    text: qsTr("Sort images")
                    display: QQC2.AbstractButton.IconOnly
                    onClicked: sortMenu.popup()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text

                    QQC2.Menu {
                        id: sortMenu

                        function choose(mode) {
                            if (root.imagesModel)
                                root.imagesModel.setSortMode(mode)
                            close()
                        }

                        QQC2.MenuItem {
                            text: qsTr("Name A–Z")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "name_asc"
                            onTriggered: sortMenu.choose("name_asc")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Name Z–A")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "name_desc"
                            onTriggered: sortMenu.choose("name_desc")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Newest First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "newest"
                            onTriggered: sortMenu.choose("newest")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Oldest First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "oldest"
                            onTriggered: sortMenu.choose("oldest")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Largest First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "largest"
                            onTriggered: sortMenu.choose("largest")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Smallest First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "smallest"
                            onTriggered: sortMenu.choose("smallest")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Used First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "used_first"
                            onTriggered: sortMenu.choose("used_first")
                        }
                        QQC2.MenuItem {
                            text: qsTr("Unused First")
                            checkable: true
                            checked: root.imagesModel && root.imagesModel.currentSortMode === "unused_first"
                            onTriggered: sortMenu.choose("unused_first")
                        }
                    }
                }

                QQC2.TextField {
                    id: searchField
                    Layout.fillWidth: true
                    placeholderText: qsTr("Search images…")
                    selectByMouse: true
                    leftPadding: Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing

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
                    text: qsTr("Refresh images")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.imagesModel && !root.imagesModel.loading
                    onClicked: root.imagesModel.refresh()
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: text
                }

                QQC2.ToolButton {
                    icon.name: "download"
                    text: qsTr("Pull image")
                    display: QQC2.AbstractButton.IconOnly
                    enabled: root.stateIs("ready") || root.stateIs("empty")
                    onClicked: root.pullRequested()
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
                if (root.imagesModel)
                    root.imagesModel.setSearchQuery(root.query)
            }
        }

        Kirigami.Separator {
            id: listSeparator
            Layout.fillWidth: true
        }

        ListView {
            id: imageList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.stateIs("ready") || root.stateIs("empty")
            activeFocusOnTab: true
            model: root.imagesModel
            currentIndex: -1
            section.property: "section"
            section.criteria: ViewSection.FullString

            section.delegate: Rectangle {
                required property string section
                width: imageList.width
                height: sectionLabel.implicitHeight + Kirigami.Units.mediumSpacing * 2
                color: Kirigami.Theme.alternateBackgroundColor

                QQC2.Label {
                    id: sectionLabel
                    anchors.left: parent.left
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.verticalCenter: parent.verticalCenter
                    text: parent.section === "in_use" ? qsTr("In Use") : qsTr("Unused")
                    font.bold: true
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            delegate: Item {
                id: imageDelegate

                required property string imageId
                required property string shortId
                required property string displayName
                required property string secondaryText
                required property string architecture
                required property bool inUse
                required property int usedByCount
                required property bool selected
                required property bool busy
                required property var repoTags
                required property string sizeText

                width: imageList.width
                implicitHeight: row.implicitHeight

                ImageListItem {
                    id: row
                    width: parent.width
                    imageId: imageDelegate.imageId
                    displayName: imageDelegate.displayName
                    secondaryText: imageDelegate.secondaryText
                    architecture: imageDelegate.architecture
                    selected: imageDelegate.selected
                    inUse: imageDelegate.inUse
                    usedByCount: imageDelegate.usedByCount
                    busy: imageDelegate.busy
                    additionalTagCount: imageDelegate.repoTags
                                        ? Math.max(0, imageDelegate.repoTags.length - 1) : 0
                    onSelectedRequested: function(id) {
                        if (root.imagesModel)
                            root.imagesModel.selectImage(id)
                    }
                    onRemoveRequested: function(id) {
                        root.requestRemoval(id, imageDelegate.displayName,
                                            imageDelegate.shortId,
                                            imageDelegate.repoTags
                                            ? imageDelegate.repoTags.join("\n") : "",
                                            imageDelegate.sizeText,
                                            imageDelegate.usedByCount)
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
        visible: root.imagesModel && root.imagesModel.loading
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
                        Layout.preferredWidth: parent.width * (skeletonRow.index % 2 === 0 ? 0.58 : 0.72)
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
        visible: root.imagesModel && !root.imagesModel.loading
                 && imageList.count === 0
                 && (root.stateIs("ready") || root.stateIs("empty"))
        icon.name: "package-x-generic"
        text: root.query.length > 0
              ? qsTr("No images match “%1”").arg(root.query)
              : qsTr("No Docker images found.")
        explanation: root.query.length > 0
                     ? qsTr("Try a different search term.")
                     : qsTr("Pull an image to get started.")
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 20)
        visible: root.stateIs("error")
        icon.name: root.imagesModel && root.imagesModel.errorKind === "permission_denied"
                   ? "dialog-password" : "network-disconnect"
        text: root.imagesModel && root.imagesModel.errorKind === "permission_denied"
              ? qsTr("TuxStack cannot access the Docker socket.")
              : (root.imagesModel && root.imagesModel.errorKind === "docker_unavailable"
                 ? qsTr("Docker Engine is unavailable.")
                 : qsTr("Images could not be loaded."))
        explanation: root.imagesModel ? root.imagesModel.errorMessage : ""

        helpfulAction: Kirigami.Action {
            text: qsTr("Retry")
            icon.name: "view-refresh"
            onTriggered: root.retryRequested()
        }
    }
}
