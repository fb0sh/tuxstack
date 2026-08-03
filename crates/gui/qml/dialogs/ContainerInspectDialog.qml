import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Read-only JSON inspection dialog (container/image inspect output).
 */
Kirigami.Dialog {
    id: root

    property string titleText: i18nd("tuxstack", "Inspect")
    property string jsonText: ""

    title: titleText

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * 40
        Layout.preferredHeight: Kirigami.Units.gridUnit * 28

        QQC2.ToolButton {
            icon.name: "edit-copy"
            text: i18nd("tuxstack", "Copy")
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
            TextArea.flickable: TextArea {
                id: jsonTextArea
                text: root.jsonText
                readOnly: true
                font.family: "monospace"
                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                selectByMouse: true
                wrapMode: TextEdit.NoWrap
            }
            ScrollBar.vertical: QQC2.ScrollBar {}
            ScrollBar.horizontal: QQC2.ScrollBar {}
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
