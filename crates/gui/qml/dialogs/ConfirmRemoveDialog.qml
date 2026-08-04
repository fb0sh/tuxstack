import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Confirmation dialog for destructive operations.
 */
Kirigami.Dialog {
    id: root

    property string titleText: I18n.i18nd("tuxstack", "Confirm")
    property string message: ""
    property string confirmText: I18n.i18nd("tuxstack", "Remove")
    property bool danger: true

    signal confirmed()

    title: titleText

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.mediumSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * 22

        QQC2.Label {
            text: root.message
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }
    }

    footer: QQC2.DialogButtonBox {
        id: buttonBox
        Layout.fillWidth: true

        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Cancel")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.reject()
        }
        QQC2.Button {
            text: root.confirmText
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: {
                root.accept()
                root.confirmed()
            }
        }
    }
}
