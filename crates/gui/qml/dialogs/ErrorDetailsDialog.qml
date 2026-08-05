import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Error details dialog (full error text).
 */
Kirigami.Dialog {
    id: root

    property string errorText: ""

    title: I18n.i18nd("tuxstack", "Error details")
    preferredWidth: Kirigami.Units.gridUnit * 32
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    contentItem: ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(errorLabel.implicitHeight,
                                             Kirigami.Units.gridUnit * 24)
            QQC2.Label {
                id: errorLabel
                width: parent.width
                text: root.errorText
                wrapMode: Text.WrapAnywhere
                color: Kirigami.Theme.negativeTextColor
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.close()
        }
    }
}
