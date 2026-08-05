import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var filesModel: null
    signal saveAsRequested(string pathToken, string name)

    title: root.filesModel && root.filesModel.previewName.length > 0
           ? root.filesModel.previewName
           : I18n.i18nd("tuxstack", "File Preview")
    preferredWidth: Kirigami.Units.gridUnit * 38
    preferredHeight: Kirigami.Units.gridUnit * 28

    onClosed: root.filesModel && root.filesModel.cancelPreview()

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.filesModel && root.filesModel.previewTruncated
            type: Kirigami.MessageType.Information
            text: I18n.i18nd("tuxstack", "Preview truncated. Save As reads the complete file through FUSE.")
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.filesModel && root.filesModel.previewBinary
            type: Kirigami.MessageType.Information
            text: I18n.i18nd("tuxstack", "Binary files are not rendered as text.")
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.filesModel && !root.filesModel.previewBinary
            clip: true

            QQC2.TextArea {
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.NoWrap
                font.family: "monospace"
                text: root.filesModel ? root.filesModel.previewText : ""
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.filesModel && root.filesModel.previewBinary
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: "application-octet-stream"
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                Layout.preferredHeight: Kirigami.Units.iconSizes.huge
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: I18n.i18nd("tuxstack", "Path: %1\nMIME: %2\nSize: %3",
                                 root.filesModel ? root.filesModel.previewPath : "",
                                 root.filesModel ? root.filesModel.previewMime : "",
                                 root.filesModel ? root.filesModel.previewSizeText : "")
                wrapMode: Text.WrapAnywhere
                horizontalAlignment: Text.AlignHCenter
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Save As…")
            icon.name: "document-save"
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.ActionRole
            onClicked: {
                if (root.filesModel)
                    root.saveAsRequested(root.filesModel.selectedEntryPath,
                                         root.filesModel.previewName)
            }
        }
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.close()
        }
    }
}
