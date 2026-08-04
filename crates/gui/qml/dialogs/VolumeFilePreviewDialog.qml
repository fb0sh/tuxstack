pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.Dialog {
    id: root

    property var filesModel: null
    signal saveAsRequested(string path)

    title: root.filesModel && root.filesModel.previewName.length > 0
           ? root.filesModel.previewName
           : I18n.i18nd("tuxstack", "Preview")
    modal: true
    standardButtons: QQC2.Dialog.Close
    width: Math.min(Kirigami.Units.gridUnit * 42, parent ? parent.width * 0.9 : Kirigami.Units.gridUnit * 42)
    height: Math.min(Kirigami.Units.gridUnit * 32, parent ? parent.height * 0.85 : Kirigami.Units.gridUnit * 32)
    anchors.centerIn: parent
    focus: true

    onClosed: {
        if (root.filesModel)
            root.filesModel.cancelPreview()
    }

    Keys.onEscapePressed: root.close()

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            Layout.fillWidth: true
            visible: root.filesModel && root.filesModel.previewTruncated
            text: I18n.i18nd("tuxstack", "Preview truncated. Showing the first portion of the file.")
            color: Kirigami.Theme.neutralTextColor
            wrapMode: Text.WordWrap
        }

        QQC2.Label {
            Layout.fillWidth: true
            visible: root.filesModel && root.filesModel.previewParseError.length > 0
                     && root.filesModel.previewIsText
            text: I18n.i18nd("tuxstack", "JSON parse failed; showing raw text. %1",
                             root.filesModel ? root.filesModel.previewParseError : "")
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.WordWrap
        }

        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.filesModel && root.filesModel.previewIsText
            clip: true

            QQC2.TextArea {
                readOnly: true
                wrapMode: TextEdit.NoWrap
                text: root.filesModel ? root.filesModel.previewText : ""
                font.family: "monospace"
                selectByMouse: true
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.filesModel && root.filesModel.previewIsImage

            Image {
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                source: root.filesModel ? root.filesModel.previewImagePath : ""
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.filesModel && root.filesModel.previewIsBinary
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "This file type is not previewed as text.")
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Name: %1", root.filesModel ? root.filesModel.previewName : "")
                Layout.fillWidth: true
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Path: %1", root.filesModel ? root.filesModel.previewPath : "")
                Layout.fillWidth: true
                wrapMode: Text.WrapAnywhere
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "MIME: %1", root.filesModel ? root.filesModel.previewMime : "")
                Layout.fillWidth: true
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Size: %1", root.filesModel ? root.filesModel.previewSizeText : "")
                Layout.fillWidth: true
            }
        }

        QQC2.BusyIndicator {
            Layout.alignment: Qt.AlignHCenter
            running: root.filesModel && root.filesModel.previewLoading
            visible: running
        }
    }

    footer: QQC2.DialogButtonBox {
        standardButtons: QQC2.DialogButtonBox.Close
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Save As…")
            icon.name: "document-save"
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.ActionRole
            onClicked: {
                if (root.filesModel)
                    root.saveAsRequested(root.filesModel.previewPath)
            }
        }
    }
}
