import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root
    property var containersModel: null
    property string groupId: ""
    property string projectName: ""
    property var targets: []
    title: I18n.i18nd("tuxstack", "Remove Group Containers")
    preferredWidth: Kirigami.Units.gridUnit * 32

    function prepare(id, name, members) {
        root.groupId = String(id)
        root.projectName = String(name || "")
        root.targets = members || []
        forceCheck.checked = false
        volumesCheck.checked = false
        open()
    }

    ColumnLayout {
        Kirigami.Heading { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Remove containers in “%1”?", root.projectName); level: 3; wrapMode: Text.WrapAnywhere }
        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentItem.childrenRect.height, Kirigami.Units.gridUnit * 14)
            ColumnLayout {
                width: parent.width
                Repeater {
                    model: root.targets
                    delegate: QQC2.Label { required property var modelData; Layout.fillWidth: true; text: "• " + String(modelData.name || modelData.id || ""); elide: Text.ElideRight }
                }
            }
        }
        Kirigami.InlineMessage { Layout.fillWidth: true; visible: true; type: Kirigami.MessageType.Warning; text: I18n.i18nd("tuxstack", "Only the listed containers are removed. Volumes, Compose configuration, project directories, and bind-mount paths are never deleted by this action.") }
        QQC2.CheckBox { id: forceCheck; text: I18n.i18nd("tuxstack", "Force remove running containers"); checked: false }
        QQC2.CheckBox { id: volumesCheck; text: I18n.i18nd("tuxstack", "Remove anonymous volumes"); checked: false }
    }
    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Remove Containers")
            icon.name: "edit-delete"
            enabled: root.containersModel && root.groupId.length > 0 && root.targets.length > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: { root.containersModel.removeGroup(root.groupId, forceCheck.checked, volumesCheck.checked); root.close() }
        }
    }
}
