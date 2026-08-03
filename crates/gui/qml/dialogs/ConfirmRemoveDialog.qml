import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Confirmation dialog for destructive operations.
 */
Kirigami.Dialog {
    id: root

    property string titleText: i18nd("tuxstack", "Confirm")
    property string message: ""
    property string confirmText: i18nd("tuxstack", "Remove")
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
            text: i18nd("tuxstack", "Cancel")
            DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.reject()
        }
        QQC2.Button {
            text: root.confirmText
            DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: {
                root.accept()
                root.confirmed()
            }
        }
    }
}
