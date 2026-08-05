pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import ".."

QQC2.ScrollView {
    id: root

    property var containersModel: null
    signal browserRequested(string url)
    signal volumeRequested(string name)
    signal networkRequested(string id, string name)
    signal hostPathRequested(string path)

    contentWidth: availableWidth
    QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

    function copyText(value) {
        clipboardBuffer.text = String(value || "")
        clipboardBuffer.selectAll()
        clipboardBuffer.copy()
        clipboardBuffer.deselect()
        clipboardBuffer.text = ""
    }

    component DetailTable: ColumnLayout {
        id: detailTable
        required property var sourceModel
        property var activate: function(row) {}
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
        Repeater {
            model: detailTable.sourceModel || []
            delegate: QQC2.ItemDelegate {
                required property var modelData
                Layout.fillWidth: true
                onClicked: detailTable.activate(modelData)
                contentItem: RowLayout {
                    QQC2.Label {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 10
                        text: String(modelData.key || modelData.name || modelData.type
                                     || modelData.containerPort || "")
                              + (modelData.containerPort && modelData.protocol
                                 ? "/" + String(modelData.protocol) : "")
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: String(modelData.value || modelData.source || modelData.ipv4
                                     || modelData.hostIp || "—")
                        wrapMode: Text.WrapAnywhere
                        textFormat: Text.PlainText
                    }
                    QQC2.Label {
                        text: String(modelData.destination || modelData.hostPort
                                     || modelData.state || "")
                        color: Kirigami.Theme.disabledTextColor
                        elide: Text.ElideRight
                    }
                    QQC2.ToolButton {
                        visible: String(modelData.browserUrl || "").length > 0
                        icon.name: "internet-web-browser"
                        text: I18n.i18nd("tuxstack", "Open in Browser")
                        display: QQC2.AbstractButton.IconOnly
                        onClicked: detailTable.activate(modelData)
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: text
                    }
                }
            }
        }
    }

    ColumnLayout {
        width: Math.max(0, root.availableWidth - Kirigami.Units.largeSpacing * 2)
        x: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            text: root.containersModel ? root.containersModel.detailName : ""
            level: 1
            wrapMode: Text.WrapAnywhere
        }

        PropertySection { Layout.fillWidth: true; title: I18n.i18nd("tuxstack", "General"); DetailTable { sourceModel: root.containersModel ? root.containersModel.generalModel : [] } }
        PropertySection { Layout.fillWidth: true; title: I18n.i18nd("tuxstack", "State"); DetailTable { sourceModel: root.containersModel ? root.containersModel.stateModel : [] } }
        PropertySection {
            Layout.fillWidth: true
            visible: root.containersModel && root.containersModel.healthModel.length > 0
            title: I18n.i18nd("tuxstack", "Health")
            DetailTable { sourceModel: root.containersModel ? root.containersModel.healthModel : [] }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Ports")
            ColumnLayout {
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; text: I18n.i18nd("tuxstack", "Container"); font.bold: true }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 4; text: I18n.i18nd("tuxstack", "Protocol"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Host IP"); font.bold: true }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: I18n.i18nd("tuxstack", "Host Port"); font.bold: true }
                    Item { Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium }
                }
                Repeater {
                    model: root.containersModel ? root.containersModel.portsModel : []
                    delegate: RowLayout {
                        id: portRow
                        required property var modelData
                        Layout.fillWidth: true
                        QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; text: String(portRow.modelData.containerPort || "—"); font.family: "monospace" }
                        QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 4; text: String(portRow.modelData.protocol || "—") }
                        QQC2.Label { Layout.fillWidth: true; text: String(portRow.modelData.hostIp || "—"); font.family: "monospace"; elide: Text.ElideMiddle }
                        QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: String(portRow.modelData.hostPort || "—"); font.family: "monospace" }
                        QQC2.ToolButton {
                            visible: String(portRow.modelData.browserUrl || "").length > 0
                            icon.name: "internet-web-browser"
                            text: I18n.i18nd("tuxstack", "Open in Browser")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: root.browserRequested(String(portRow.modelData.browserUrl))
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                    }
                }
            }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Mounts")
            ColumnLayout {
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: I18n.i18nd("tuxstack", "Type"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Source"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Destination"); font.bold: true }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: I18n.i18nd("tuxstack", "Access"); font.bold: true }
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; text: I18n.i18nd("tuxstack", "Propagation"); font.bold: true }
                }
                Repeater {
                    model: root.containersModel ? root.containersModel.mountsModel : []
                    delegate: QQC2.ItemDelegate {
                        id: mountRow
                        required property var modelData
                        Layout.fillWidth: true
                        enabled: String(modelData.volumeName || "").length > 0
                                 || (root.containersModel.localEndpoint
                                     && String(modelData.type || "") === "bind")
                        onClicked: {
                            if (String(modelData.volumeName || "").length > 0)
                                root.volumeRequested(String(modelData.volumeName))
                            else if (root.containersModel.localEndpoint
                                     && String(modelData.source || "").length > 0)
                                root.hostPathRequested(String(modelData.source))
                        }
                        contentItem: RowLayout {
                            QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: String(mountRow.modelData.type || "—") }
                            QQC2.Label { Layout.fillWidth: true; text: String(mountRow.modelData.source || "—"); font.family: "monospace"; elide: Text.ElideMiddle; QQC2.ToolTip.visible: truncated; QQC2.ToolTip.text: text }
                            QQC2.Label { Layout.fillWidth: true; text: String(mountRow.modelData.destination || "—"); font.family: "monospace"; elide: Text.ElideMiddle; QQC2.ToolTip.visible: truncated; QQC2.ToolTip.text: text }
                            QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 5; text: String(mountRow.modelData.access || "—") }
                            QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; text: String(mountRow.modelData.propagation || "—"); elide: Text.ElideRight }
                        }
                    }
                }
            }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Networks")
            ColumnLayout {
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 7; text: I18n.i18nd("tuxstack", "Name"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "IPv4 / IPv6"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Gateway"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "MAC"); font.bold: true }
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Aliases"); font.bold: true }
                }
                Repeater {
                    model: root.containersModel ? root.containersModel.networksModel : []
                    delegate: QQC2.ItemDelegate {
                        id: networkRow
                        required property var modelData
                        Layout.fillWidth: true
                        onClicked: root.networkRequested(String(modelData.id || ""), String(modelData.name || ""))
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: I18n.i18nd("tuxstack", "Endpoint ID: %1", String(modelData.endpointId || "—"))
                        contentItem: RowLayout {
                            QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 7; text: String(networkRow.modelData.name || "—"); font.bold: true; elide: Text.ElideRight }
                            QQC2.Label { Layout.fillWidth: true; text: [String(networkRow.modelData.ipv4 || ""), String(networkRow.modelData.ipv6 || "")].filter(Boolean).join(" / ") || "—"; font.family: "monospace"; elide: Text.ElideMiddle }
                            QQC2.Label { Layout.fillWidth: true; text: String(networkRow.modelData.gateway || "—"); font.family: "monospace"; elide: Text.ElideMiddle }
                            QQC2.Label { Layout.fillWidth: true; text: String(networkRow.modelData.mac || "—"); font.family: "monospace"; elide: Text.ElideMiddle }
                            QQC2.Label { Layout.fillWidth: true; text: String(networkRow.modelData.aliases || "—"); elide: Text.ElideRight }
                        }
                    }
                }
            }
        }
        PropertySection { Layout.fillWidth: true; title: I18n.i18nd("tuxstack", "Configuration"); DetailTable { sourceModel: root.containersModel ? root.containersModel.configurationModel : [] } }

        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Environment")
            ColumnLayout {
                Layout.fillWidth: true
                Repeater {
                    model: root.containersModel ? root.containersModel.environmentModel : []
                    delegate: RowLayout {
                        id: environmentRow
                        required property var modelData
                        Layout.fillWidth: true
                        QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 12; text: String(modelData.key || ""); font.family: "monospace"; elide: Text.ElideRight }
                        QQC2.Label { Layout.fillWidth: true; text: String(modelData.value || ""); font.family: "monospace"; elide: Text.ElideRight; textFormat: Text.PlainText }
                        QQC2.ToolButton {
                            icon.name: "edit-copy"
                            text: I18n.i18nd("tuxstack", "Copy key")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: root.copyText(environmentRow.modelData.key)
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                        QQC2.ToolButton {
                            icon.name: "edit-copy-path"
                            text: Boolean(environmentRow.modelData.revealed)
                                  ? I18n.i18nd("tuxstack", "Copy value")
                                  : I18n.i18nd("tuxstack", "Reveal the value before copying")
                            enabled: Boolean(environmentRow.modelData.revealed)
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: root.copyText(environmentRow.modelData.value)
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                        QQC2.ToolButton {
                            icon.name: Boolean(environmentRow.modelData.revealed) ? "visibility" : "hint"
                            text: Boolean(environmentRow.modelData.revealed) ? I18n.i18nd("tuxstack", "Hide value") : I18n.i18nd("tuxstack", "Reveal value")
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: {
                                if (Boolean(environmentRow.modelData.revealed)) root.containersModel.concealEnvironment(Number(environmentRow.modelData.index))
                                else root.containersModel.revealEnvironment(Number(environmentRow.modelData.index))
                            }
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: text
                        }
                    }
                }
            }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Labels")
            KeyValueTable {
                Layout.fillWidth: true
                sourceModel: root.containersModel ? root.containersModel.labelsModel : []
                totalCount: root.containersModel ? root.containersModel.labelsModel.length : 0
                searchable: totalCount >= 8
                emptyText: I18n.i18nd("tuxstack", "No labels.")
                noMatchesText: I18n.i18nd("tuxstack", "No matching labels.")
                searchPlaceholder: I18n.i18nd("tuxstack", "Search labels…")
                onSearchRequested: function(query) { root.containersModel.setLabelSearch(query) }
                onSortRequested: function(ascending) { root.containersModel.setLabelSortAscending(ascending) }
            }
        }
        Item { Layout.fillWidth: true; Layout.preferredHeight: Kirigami.Units.largeSpacing }
    }

    QQC2.TextArea {
        id: clipboardBuffer
        visible: false
    }
}
