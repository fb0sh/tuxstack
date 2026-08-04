pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var networksModel: null
    readonly property var detail: root.networksModel ? root.networksModel.detail : null
    readonly property string detailState: root.networksModel
                                          ? String(root.networksModel.detailState).toLowerCase()
                                          : "none"

    function text(value) {
        return value === undefined || value === null || String(value).length === 0
               ? I18n.i18nd("tuxstack", "—") : String(value)
    }

    function booleanText(value) {
        return value ? I18n.i18nd("tuxstack", "true")
                     : I18n.i18nd("tuxstack", "false")
    }

    function modelCount(value) {
        return value && typeof value.length !== "undefined" ? value.length : 0
    }

    QQC2.ScrollView {
        anchors.fill: parent
        visible: root.networksModel
                 && root.networksModel.selectedNetworkId.length > 0
                 && root.detailState === "ready" && root.detail
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
                text: root.detail ? root.text(root.detail.name) : ""
                level: 1
                wrapMode: Text.Wrap
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "General")

                PropertyList {
                    Layout.fillWidth: true

                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Name")
                        value: root.detail ? root.text(root.detail.name)
                                           : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "ID")
                        value: root.detail ? root.text(root.detail.networkId)
                                           : I18n.i18nd("tuxstack", "—")
                        copyable: root.detail && String(root.detail.networkId).length > 0
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Created")
                        value: root.detail ? root.text(root.detail.createdText)
                                           : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Driver")
                        value: root.detail ? root.text(root.detail.driver)
                                           : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Scope")
                        value: root.detail ? root.text(root.detail.scope)
                                           : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Subnet")
                        value: root.detail ? root.text(root.detail.subnet)
                                           : I18n.i18nd("tuxstack", "—")
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Gateway")
                        value: root.detail ? root.text(root.detail.gateway)
                                           : I18n.i18nd("tuxstack", "—")
                        monospace: true
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Internal")
                        value: root.detail
                               ? root.booleanText(Boolean(root.detail.internal))
                               : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Attachable")
                        value: root.detail
                               ? root.booleanText(Boolean(root.detail.attachable))
                               : I18n.i18nd("tuxstack", "—")
                    }
                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Ingress")
                        value: root.detail
                               ? root.booleanText(Boolean(root.detail.ingress))
                               : I18n.i18nd("tuxstack", "—")
                    }
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Options")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.networksModel ? root.networksModel.optionRows : null
                    totalCount: root.networksModel
                                ? root.modelCount(root.networksModel.optionRows) : 0
                    searchable: false
                    emptyText: I18n.i18nd("tuxstack", "No network options.")
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "IPAM")

                PropertyList {
                    Layout.fillWidth: true

                    PropertyRow {
                        Layout.fillWidth: true
                        label: I18n.i18nd("tuxstack", "Driver")
                        value: root.detail ? root.text(root.detail.ipamDriver)
                                           : I18n.i18nd("tuxstack", "—")
                    }
                }

                Kirigami.Heading {
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.mediumSpacing
                    visible: subnetRepeater.count > 0
                    text: I18n.i18nd("tuxstack", "Subnets")
                    level: 3
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    Repeater {
                        id: subnetRepeater
                        model: root.networksModel ? root.networksModel.subnetRows : null

                        delegate: PropertyList {
                            id: subnetDelegate
                            required property var model
                            Layout.fillWidth: true

                            PropertyRow {
                                Layout.fillWidth: true
                                label: I18n.i18nd("tuxstack", "Subnet")
                                value: root.text(subnetDelegate.model.subnet)
                                copyable: value !== I18n.i18nd("tuxstack", "—")
                                monospace: true
                            }
                            PropertyRow {
                                Layout.fillWidth: true
                                label: I18n.i18nd("tuxstack", "Gateway")
                                value: root.text(subnetDelegate.model.gateway)
                                copyable: value !== I18n.i18nd("tuxstack", "—")
                                monospace: true
                            }
                        }
                    }
                }

                QQC2.Label {
                    Layout.fillWidth: true
                    visible: subnetRepeater.count === 0
                    text: I18n.i18nd("tuxstack", "No IPAM subnets.")
                    color: Kirigami.Theme.disabledTextColor
                    wrapMode: Text.Wrap
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Containers")

                NetworkContainerList {
                    Layout.fillWidth: true
                    sourceModel: root.networksModel
                                 ? root.networksModel.containerRows : null
                }
            }

            PropertySection {
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Labels")

                KeyValueTable {
                    Layout.fillWidth: true
                    sourceModel: root.networksModel ? root.networksModel.labelRows : null
                    totalCount: root.networksModel
                                ? root.modelCount(root.networksModel.labelRows) : 0
                    searchable: false
                    emptyText: I18n.i18nd("tuxstack", "No labels.")
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
        visible: root.networksModel
                 && root.networksModel.selectedNetworkId.length > 0
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
                    model: skeletonSection.index === 0 ? 7 : 3

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
        visible: root.networksModel
                 && root.networksModel.selectedNetworkId.length > 0
                 && root.detailState === "error"
        icon.name: "dialog-error"
        text: I18n.i18nd("tuxstack", "Network details unavailable")
        explanation: root.networksModel && root.networksModel.detailError.length > 0
                     ? I18n.i18nd("tuxstack",
                                  "Failed to load network information.\n\n%1")
                       .arg(root.networksModel.detailError)
                     : I18n.i18nd("tuxstack",
                                  "Failed to load network information.")

        helpfulAction: Kirigami.Action {
            text: I18n.i18nd("tuxstack", "Retry")
            icon.name: "view-refresh"
            onTriggered: {
                if (root.networksModel)
                    root.networksModel.reloadSelectedNetwork()
            }
        }
    }
}
