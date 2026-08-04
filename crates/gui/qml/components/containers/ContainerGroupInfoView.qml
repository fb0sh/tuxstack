import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import ".."

QQC2.ScrollView {
    id: root

    property var containersModel: null
    signal containerRequested(string id)
    signal projectFolderRequested(string path)

    contentWidth: availableWidth
    QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

    ColumnLayout {
        width: Math.max(0, root.availableWidth - Kirigami.Units.largeSpacing * 2)
        x: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            text: root.containersModel ? root.containersModel.groupProjectName : ""
            level: 1
            wrapMode: Text.WrapAnywhere
        }

        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Group")
            PropertyList {
                Layout.fillWidth: true
                PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Project Name"); value: root.containersModel ? root.containersModel.groupProjectName : "—"; copyable: true }
                PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Status"); value: root.containersModel ? root.containersModel.groupStatus : "—" }
                PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Working Directory"); value: root.containersModel && root.containersModel.groupWorkingDirectory.length > 0 ? root.containersModel.groupWorkingDirectory : "—"; copyable: value !== "—"; monospace: true }
                PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Compose Files"); value: root.containersModel && root.containersModel.groupComposeFiles.length > 0 ? root.containersModel.groupComposeFiles : "—"; copyable: value !== "—"; monospace: true }
                PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Compose Version"); value: root.containersModel && root.containersModel.groupComposeVersion.length > 0 ? root.containersModel.groupComposeVersion : "—" }
            }
            QQC2.Button {
                visible: root.containersModel && root.containersModel.localEndpoint
                         && root.containersModel.groupWorkingDirectory.length > 0
                text: I18n.i18nd("tuxstack", "Open Project Folder")
                icon.name: "folder-open"
                onClicked: root.projectFolderRequested(root.containersModel.groupWorkingDirectory)
            }
        }

        PropertySection {
            Layout.fillWidth: true
            title: I18n.i18nd("tuxstack", "Containers")
            ColumnLayout {
                Layout.fillWidth: true
                Repeater {
                    model: root.containersModel ? root.containersModel.groupMembersModel : []
                    delegate: QQC2.ItemDelegate {
                        required property var modelData
                        Layout.fillWidth: true
                        onClicked: root.containerRequested(String(modelData.id || ""))
                        contentItem: RowLayout {
                            QQC2.Label { Layout.fillWidth: true; text: String(modelData.name || ""); font.bold: true; elide: Text.ElideRight }
                            QQC2.Label { text: String(modelData.service || ""); color: Kirigami.Theme.disabledTextColor }
                            QQC2.Label { text: String(modelData.state || "") }
                            QQC2.Label { Layout.preferredWidth: Kirigami.Units.gridUnit * 10; text: String(modelData.image || ""); elide: Text.ElideRight; color: Kirigami.Theme.disabledTextColor }
                        }
                    }
                }
            }
        }
        Item { Layout.fillWidth: true; Layout.preferredHeight: Kirigami.Units.largeSpacing }
    }
}
