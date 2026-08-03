import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Error details dialog (full error text).
 */
Kirigami.Dialog {
    id: root

    property string errorText: ""

    title: i18nd("tuxstack", "Error details")

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * 32

        QQC2.Label {
            text: root.errorText
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.negativeTextColor
            Layout.fillWidth: true
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: i18nd("tuxstack", "Close")
            DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.close()
        }
    }
}
