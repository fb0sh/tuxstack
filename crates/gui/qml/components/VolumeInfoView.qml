pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var volumesModel: null
    readonly property string detailState: root.volumesModel
                                          ? String(root.volumesModel.detailState).toLowerCase()
                                          : "none"

    signal exportRequested(string volumeName)
    signal cloneRequested(string volumeName)
    signal containerRequested(string containerId)

    function value(value, unknownText) {
        if (value === undefined || value === null || String(value).length === 0)
            return unknownText
        return String(value)
    }

    QQC2.ScrollView {
        anchors.fill: parent
        visible: root.volumesModel
                 && root.volumesModel.selectedVolumeName.length > 0
                 && root.detailState === "ready"
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
                text: root.volumesModel ? root.volumesModel.detailName : ""
                level: 1
                wrapMode: Text.WrapAnywhere
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "General")

                PropertyList {
                    Layout.fillWidth: true

                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Name")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailName,
                                            I18n.i18nd("tuxstack", "—"))
                               : I18n.i18nd("tuxstack", "—")
                        copyable: root.volumesModel
                                  && root.volumesModel.detailName.length > 0
                        toolTipText: value
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Driver")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailDriver,
                                            I18n.i18nd("tuxstack", "—"))
                               : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Scope")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailScope,
                                            I18n.i18nd("tuxstack", "—"))
                               : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Mountpoint")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailMountpoint,
                                            I18n.i18nd("tuxstack", "—"))
                               : I18n.i18nd("tuxstack", "—")
                        copyable: root.volumesModel
                                  && root.volumesModel.detailMountpoint.length > 0
                        monospace: true
                        toolTipText: value
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Created")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailCreatedText,
                                            I18n.i18nd("tuxstack", "—"))
                               : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Size")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailSizeText,
                                            I18n.i18nd("tuxstack", "Unknown"))
                               : I18n.i18nd("tuxstack", "Unknown")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Reference Count")
                        value: root.volumesModel
                               ? root.value(root.volumesModel.detailRefCountText,
                                            I18n.i18nd("tuxstack", "Unknown"))
                               : I18n.i18nd("tuxstack", "Unknown")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Anonymous")
                        value: root.volumesModel && root.volumesModel.detailAnonymous
                               ? I18n.i18nd("tuxstack", "Yes")
                               : I18n.i18nd("tuxstack", "No")
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Actions")

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.mediumSpacing

                    QQC2.Button {
                        text: I18n.i18nd("tuxstack", "Export Volume…")
                        icon.name: "document-export"
                        enabled: root.volumesModel
                                 && !root.volumesModel.selectedVolumeBusy
                        focusPolicy: Qt.StrongFocus
                        onClicked: root.exportRequested(root.volumesModel.detailName)
                    }
                    QQC2.Button {
                        text: I18n.i18nd("tuxstack", "Clone Volume…")
                        icon.name: "edit-copy"
                        enabled: root.volumesModel
                                 && !root.volumesModel.selectedVolumeBusy
                        focusPolicy: Qt.StrongFocus
                        onClicked: root.cloneRequested(root.volumesModel.detailName)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Used By")

                VolumeUsedByList {
                    Layout.fillWidth: true
                    sourceModel: root.volumesModel
                                 ? root.volumesModel.usedByModel : null
                    onContainerRequested: function(containerId) {
                        root.containerRequested(containerId)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Labels")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.volumesModel ? root.volumesModel.labelModel : null
                    totalCount: root.volumesModel
                                ? Number(root.volumesModel.labelCount) : 0
                    searchable: totalCount >= 8
                    emptyText: I18n.i18nd("tuxstack", "No labels.")
                    noMatchesText: I18n.i18nd("tuxstack", "No matching labels.")
                    searchPlaceholder: I18n.i18nd("tuxstack", "Search labels…")
                    onSearchRequested: function(query) {
                        if (root.volumesModel)
                            root.volumesModel.setLabelSearchQuery(query)
                    }
                    onSortRequested: function(ascending) {
                        if (root.volumesModel)
                            root.volumesModel.setLabelSortAscending(ascending)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Driver Options")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.volumesModel ? root.volumesModel.optionModel : null
                    totalCount: root.volumesModel
                                ? Number(root.volumesModel.optionCount) : 0
                    searchable: totalCount >= 8
                    emptyText: I18n.i18nd("tuxstack", "No driver options.")
                    noMatchesText: I18n.i18nd("tuxstack", "No matching driver options.")
                    searchPlaceholder: I18n.i18nd("tuxstack", "Search driver options…")
                    onSearchRequested: function(query) {
                        if (root.volumesModel)
                            root.volumesModel.setOptionSearchQuery(query)
                    }
                    onSortRequested: function(ascending) {
                        if (root.volumesModel)
                            root.volumesModel.setOptionSortAscending(ascending)
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                visible: root.volumesModel
                         && Number(root.volumesModel.statusCount) > 0
                title: I18n.i18nd("tuxstack", "Status")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.volumesModel ? root.volumesModel.statusModel : null
                    totalCount: root.volumesModel
                                ? Number(root.volumesModel.statusCount) : 0
                    searchable: totalCount >= 8
                    emptyText: I18n.i18nd("tuxstack", "No status information.")
                    noMatchesText: I18n.i18nd("tuxstack", "No matching status entries.")
                    searchPlaceholder: I18n.i18nd("tuxstack", "Search status…")
                    onSearchRequested: function(query) {
                        if (root.volumesModel)
                            root.volumesModel.setStatusSearchQuery(query)
                    }
                    onSortRequested: function(ascending) {
                        if (root.volumesModel)
                            root.volumesModel.setStatusSortAscending(ascending)
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
        visible: root.volumesModel
                 && root.volumesModel.selectedVolumeName.length > 0
                 && root.detailState === "loading"
        spacing: Kirigami.Units.largeSpacing

        Repeater {
            model: 3
            delegate: ColumnLayout {
                id: skeletonSection
                required property int index
                Layout.fillWidth: true
                spacing: Kirigami.Units.mediumSpacing

                Rectangle {
                    Layout.preferredWidth: Kirigami.Units.gridUnit
                                           * (skeletonSection.index === 0 ? 8 : 11)
                    Layout.preferredHeight: Kirigami.Units.gridUnit
                    radius: Kirigami.Units.smallSpacing
                    color: Kirigami.Theme.alternateBackgroundColor
                }
                Repeater {
                    model: skeletonSection.index === 0 ? 8 : 3
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
        visible: root.volumesModel
                 && root.volumesModel.selectedVolumeName.length > 0
                 && root.detailState === "error"
        icon.name: "dialog-error"
        text: I18n.i18nd("tuxstack", "Volume details unavailable")
        explanation: root.volumesModel && root.volumesModel.detailError.length > 0
                     ? I18n.i18nd("tuxstack", "Failed to load volume information.\n\n%1")
                       .arg(root.volumesModel.detailError)
                     : I18n.i18nd("tuxstack", "Failed to load volume information.")

        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: {
                if (root.volumesModel)
                    root.volumesModel.reloadSelectedVolume()
            }
        }
    }
}
