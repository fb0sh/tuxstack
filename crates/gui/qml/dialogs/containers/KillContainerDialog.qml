import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root
    property var containersModel: null
    property string containerId: ""
    property string name: ""
    title: I18n.i18nd("tuxstack", "Kill Container")
    preferredWidth: Kirigami.Units.gridUnit * 26
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    function prepare(id, currentName) { root.containerId = String(id); root.name = String(currentName || id); open() }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing
        Kirigami.Heading { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Kill “%1”?", root.name); level: 3; wrapMode: Text.WrapAnywhere }
        Kirigami.InlineMessage { Layout.fillWidth: true; visible: true; type: Kirigami.MessageType.Warning; text: I18n.i18nd("tuxstack", "Docker will send SIGKILL immediately. The container cannot shut down gracefully.") }
    }
    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Kill Container")
            icon.name: "process-stop"
            enabled: root.containersModel && root.containerId.length > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: { root.containersModel.killContainer(root.containerId); root.close() }
        }
    }
}
