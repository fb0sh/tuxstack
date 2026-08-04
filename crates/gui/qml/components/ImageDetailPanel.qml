import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    property var imagesModel: null
    readonly property var detail: imagesModel ? imagesModel.detail : null
    readonly property string detailState: imagesModel
                                          ? String(imagesModel.detailState).toLowerCase()
                                          : "none"
    property bool tagsExpanded: false

    signal exportRequested(string imageId, string displayName, string shortId)
    signal containerRequested(string containerId)

    function text(value) {
        return value === undefined || value === null || String(value).length === 0
               ? qsTr("—") : String(value)
    }

    function tagsSummary() {
        if (!root.detail || !root.detail.repoTags || root.detail.repoTags.length === 0)
            return qsTr("—")
        if (root.tagsExpanded || root.detail.repoTags.length === 1)
            return root.detail.repoTags.join("\n")
        return String(root.detail.repoTags[0])
    }

    function modelCount(value) {
        return value && typeof value.length !== "undefined" ? value.length : 0
    }

    onDetailChanged: tagsExpanded = false

    QQC2.ScrollView {
        anchors.fill: parent
        visible: root.detailState === "ready" && root.detail
        contentWidth: availableWidth
        QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

        ColumnLayout {
            width: Math.min(root.width - Kirigami.Units.largeSpacing * 2,
                            Kirigami.Units.gridUnit * 50)
            x: Math.max(Kirigami.Units.largeSpacing, (root.width - width) / 2)
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Heading {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.largeSpacing
                text: root.detail ? root.text(root.detail.displayName) : ""
                level: 1
                wrapMode: Text.Wrap
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("General")

                PropertyList {
                    Layout.fillWidth: true

                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("ID")
                        value: root.detail ? root.text(root.detail.imageId) : qsTr("—")
                        copyable: root.detail && root.detail.imageId.length > 0
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Tags")
                        value: root.tagsSummary()
                        copyValue: root.detail ? root.detail.tagsText : ""
                        copyable: root.detail && root.detail.repoTags
                                  && root.detail.repoTags.length > 0
                        monospace: true
                        expandable: root.detail && root.detail.repoTags
                                    && root.detail.repoTags.length > 1
                        expanded: root.tagsExpanded
                        expandText: root.detail && root.detail.repoTags
                                    ? qsTr("+%1 more").arg(root.detail.repoTags.length - 1)
                                    : qsTr("Show more")
                        onExpansionRequested: function(expanded) {
                            root.tagsExpanded = expanded
                        }
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Created")
                        value: root.detail ? root.text(root.detail.createdText) : qsTr("—")
                        toolTipText: root.detail ? root.text(root.detail.createdFullText) : ""
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Size")
                        value: root.detail ? root.text(root.detail.sizeText) : qsTr("—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Platform")
                        value: root.detail ? root.text(root.detail.platform) : qsTr("—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Architecture")
                        value: root.detail ? root.text(root.detail.architecture) : qsTr("—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("OS")
                        value: root.detail ? root.text(root.detail.os) : qsTr("—")
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("Actions")

                QQC2.Button {
                    text: qsTr("Export Image…")
                    icon.name: "document-save"
                    enabled: root.detail && !root.imagesModel.exporting
                    onClicked: root.exportRequested(root.detail.imageId,
                                                    root.detail.displayName,
                                                    root.detail.shortId)
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("Configuration")

                PropertyList {
                    Layout.fillWidth: true

                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Command")
                        value: root.detail ? root.text(root.detail.commandText) : qsTr("—")
                        copyable: root.detail && root.detail.commandText.length > 0
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Entrypoint")
                        value: root.detail ? root.text(root.detail.entrypointText) : qsTr("—")
                        copyable: root.detail && root.detail.entrypointText.length > 0
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Working Directory")
                        value: root.detail ? root.text(root.detail.workingDir) : qsTr("—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("User")
                        value: root.detail ? root.text(root.detail.user) : qsTr("—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: qsTr("Stop Signal")
                        value: root.detail ? root.text(root.detail.stopSignal) : qsTr("—")
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("Environment")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.imagesModel ? root.imagesModel.environmentModel : null
                    totalCount: root.imagesModel
                                ? root.modelCount(root.imagesModel.environmentRows) : 0
                    emptyText: qsTr("No environment variables.")
                    searchPlaceholder: qsTr("Search environment…")
                    onSearchRequested: function(query) {
                        if (root.imagesModel)
                            root.imagesModel.setEnvironmentSearchQuery(query)
                    }
                    onSortRequested: function(ascending) {
                        if (root.imagesModel)
                            root.imagesModel.setEnvironmentSortAscending(ascending)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("Labels")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.imagesModel ? root.imagesModel.labelModel : null
                    totalCount: root.imagesModel
                                ? root.modelCount(root.imagesModel.labelRows) : 0
                    emptyText: qsTr("No labels.")
                    searchPlaceholder: qsTr("Search labels…")
                    onSearchRequested: function(query) {
                        if (root.imagesModel)
                            root.imagesModel.setLabelSearchQuery(query)
                    }
                    onSortRequested: function(ascending) {
                        if (root.imagesModel)
                            root.imagesModel.setLabelSortAscending(ascending)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: qsTr("Used By")

                ImageUsedByList {
                    Layout.fillWidth: true
                    sourceModel: root.imagesModel ? root.imagesModel.usageModel : null
                    onContainerRequested: function(containerId) {
                        root.containerRequested(containerId)
                    }
                }
            }

            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.largeSpacing
            }
        }
    }

    ColumnLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Kirigami.Units.largeSpacing
        visible: root.imagesModel
                 && root.imagesModel.selectedImageId.length > 0
                 && root.detailState === "loading"
        spacing: Kirigami.Units.largeSpacing

        Repeater {
            model: 2
            delegate: ColumnLayout {
                id: skeletonSection
                required property int index
                Layout.fillWidth: true
                spacing: Kirigami.Units.mediumSpacing

                Rectangle {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * (skeletonSection.index === 0 ? 8 : 12)
                    Layout.preferredHeight: Kirigami.Units.gridUnit
                    radius: Kirigami.Units.smallSpacing
                    color: Kirigami.Theme.alternateBackgroundColor
                }

                Repeater {
                    model: skeletonSection.index === 0 ? 7 : 5
                    delegate: RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.largeSpacing

                        Rectangle {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 7
                            Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
                            radius: Kirigami.Units.smallSpacing
                            color: Kirigami.Theme.alternateBackgroundColor
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
                            radius: Kirigami.Units.smallSpacing
                            color: Kirigami.Theme.alternateBackgroundColor
                        }
                    }
                }
            }
        }
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 24)
        visible: root.detailState === "error"
        icon.name: "dialog-error"
        text: qsTr("Image details unavailable")
        explanation: root.imagesModel && root.imagesModel.detailError.length > 0
                     ? qsTr("Failed to load image information.\n\n%1")
                       .arg(root.imagesModel.detailError)
                     : qsTr("Failed to load image information.")

        helpfulAction: Kirigami.Action {
            text: qsTr("Retry")
            icon.name: "view-refresh"
            onTriggered: {
                if (root.imagesModel)
                    root.imagesModel.reloadSelectedImage()
            }
        }
    }
}
