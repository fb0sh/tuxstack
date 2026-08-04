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
            DetailTable {
                sourceModel: root.containersModel ? root.containersModel.portsModel : []
                activate: function(row) { if (String(row.browserUrl || "").length > 0) root.browserRequested(String(row.browserUrl)) }
            }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Mounts")
            DetailTable {
                sourceModel: root.containersModel ? root.containersModel.mountsModel : []
                activate: function(row) {
                    if (String(row.volumeName || "").length > 0) root.volumeRequested(String(row.volumeName))
                    else if (root.containersModel.localEndpoint
                             && String(row.type || "") === "bind"
                             && String(row.source || "").length > 0)
                        root.hostPathRequested(String(row.source))
                }
            }
        }
        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Networks")
            DetailTable {
                sourceModel: root.containersModel ? root.containersModel.networksModel : []
                activate: function(row) { root.networkRequested(String(row.id || ""), String(row.name || "")) }
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
}
