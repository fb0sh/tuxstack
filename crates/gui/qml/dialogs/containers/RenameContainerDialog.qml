import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root
    property var containersModel: null
    property string containerId: ""
    title: I18n.i18nd("tuxstack", "Rename Container")
    preferredWidth: Kirigami.Units.gridUnit * 26

    function prepare(id, currentName) {
        root.containerId = String(id)
        nameField.text = String(currentName || "")
        open()
        nameField.forceActiveFocus()
        nameField.selectAll()
    }

    ColumnLayout {
        QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "New container name") }
        QQC2.TextField { id: nameField; Layout.fillWidth: true; selectByMouse: true; validator: RegularExpressionValidator { regularExpression: /[A-Za-z0-9][A-Za-z0-9_.-]*/ } }
    }
    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Rename")
            enabled: root.containersModel && root.containerId.length > 0 && nameField.acceptableInput && nameField.text.trim().length > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: { root.containersModel.renameContainer(root.containerId, nameField.text.trim()); root.close() }
        }
    }
}
