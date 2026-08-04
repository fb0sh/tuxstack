import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Read-only JSON inspection dialog (container/image inspect output).
 */
Kirigami.Dialog {
    id: root

    property string titleText: I18n.i18nd("tuxstack", "Inspect")
    property string jsonText: ""

    title: titleText

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * 40
        Layout.preferredHeight: Kirigami.Units.gridUnit * 28

        QQC2.ToolButton {
            icon.name: "edit-copy"
            text: I18n.i18nd("tuxstack", "Copy")
            onClicked: {
                // Simple clipboard write via selection
                root.jsonTextArea.selectAll()
                root.jsonTextArea.copy()
                root.jsonTextArea.deselect()
            }
        }

        Flickable {
            id: flick
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: jsonTextArea.implicitWidth
            contentHeight: jsonTextArea.implicitHeight
            QQC2.TextArea.flickable: QQC2.TextArea {
                id: jsonTextArea
                text: root.jsonText
                readOnly: true
                font.family: "monospace"
                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                selectByMouse: true
                wrapMode: TextEdit.NoWrap
            }
            QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
            QQC2.ScrollBar.horizontal: QQC2.ScrollBar {}
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
