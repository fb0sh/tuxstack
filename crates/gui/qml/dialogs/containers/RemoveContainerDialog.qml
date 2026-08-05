import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../../components"

Kirigami.Dialog {
    id: root

    property var containersModel: null
    property string containerId: ""
    property string name: ""
    property string image: ""
    property string state: ""
    property string composeProject: ""
    property var mounts: []

    title: I18n.i18nd("tuxstack", "Remove Container")
    preferredWidth: Kirigami.Units.gridUnit * 32
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    function prepare(id, nameValue, imageValue, stateValue, composeValue, mountRows) {
        root.containerId = String(id)
        root.name = String(nameValue || id)
        root.image = String(imageValue || "")
        root.state = String(stateValue || "")
        root.composeProject = String(composeValue || "")
        root.mounts = mountRows || []
        forceCheck.checked = false
        volumesCheck.checked = false
        open()
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing
        Kirigami.Heading { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Remove “%1”?", root.name); level: 3; wrapMode: Text.WrapAnywhere }
        PropertyList {
            Layout.fillWidth: true
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Name"); value: root.name; copyable: true }
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "ID"); value: root.containerId; copyable: true; monospace: true }
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Image"); value: root.image.length > 0 ? root.image : "—" }
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "State"); value: root.state.length > 0 ? root.state : "—" }
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Compose Project"); value: root.composeProject.length > 0 ? root.composeProject : "—" }
            PropertyRow { Layout.fillWidth: true; label: I18n.i18nd("tuxstack", "Mounts"); value: root.mounts.length > 0 ? I18n.i18nd("tuxstack", "%1 mounts", root.mounts.length) : I18n.i18nd("tuxstack", "No mounts") }
        }
        Kirigami.InlineMessage { Layout.fillWidth: true; visible: true; type: Kirigami.MessageType.Warning; text: I18n.i18nd("tuxstack", "This removes the container only. Named volumes, bind-mount paths, Compose files, and project directories are not deleted.") }
        QQC2.CheckBox { id: forceCheck; text: I18n.i18nd("tuxstack", "Force remove running container"); checked: false }
        QQC2.CheckBox { id: volumesCheck; text: I18n.i18nd("tuxstack", "Remove anonymous volumes"); checked: false }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Remove Container")
            icon.name: "edit-delete"
            enabled: root.containersModel && root.containerId.length > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: { root.containersModel.removeContainer(root.containerId, forceCheck.checked, volumesCheck.checked); root.close() }
        }
    }
}
