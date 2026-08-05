import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs as Dialogs
import QtQuick.Layouts
import QtCore as QtCore
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var volumesModel: null
    property string volumeName: ""
    property bool submitted: false
    property bool cancelling: false
    readonly property bool exporting: root.volumesModel
                                      && root.volumesModel.exportingVolumeName === root.volumeName

    title: I18n.i18nd("tuxstack", "Export Volume")
    preferredWidth: Kirigami.Units.gridUnit * 30
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing
    closePolicy: root.exporting ? QQC2.Popup.NoAutoClose
                                : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function safeName(value) {
        let result = String(value).replace(/[^A-Za-z0-9._-]+/g, "-")
                                  .replace(/^-+|-+$/g, "")
        return result.length > 0 ? result : "volume"
    }

    function extension() {
        if (formatBox.currentValue === "tar_gzip")
            return ".tar.gz"
        if (formatBox.currentValue === "tar_zstd")
            return ".tar.zst"
        return ".tar"
    }

    function prepare(name) {
        root.volumeName = String(name)
        root.submitted = false
        root.cancelling = false
        formatModel.clear()
        formatModel.append({"label": I18n.i18nd("tuxstack", "Tar archive (.tar)"),
                            "value": "tar"})
        formatModel.append({"label": I18n.i18nd("tuxstack", "Gzip-compressed tar archive (.tar.gz)"),
                            "value": "tar_gzip"})
        if (root.volumesModel && root.volumesModel.zstdAvailable) {
            formatModel.append({"label": I18n.i18nd("tuxstack", "Zstandard-compressed tar archive (.tar.zst)"),
                                "value": "tar_zstd"})
        }
        formatBox.currentIndex = 0
        const folder = QtCore.StandardPaths.writableLocation(QtCore.StandardPaths.DocumentsLocation)
        destinationField.text = folder + "/" + root.safeName(root.volumeName) + root.extension()
        open()
    }

    function ensureExtension(path) {
        const suffix = root.extension()
        const lower = path.toLowerCase()
        if (lower.endsWith(suffix))
            return path
        return path + suffix
    }

    function beginExport() {
        if (!root.volumesModel || root.exporting || destinationField.text.trim().length === 0)
            return
        root.submitted = true
        root.cancelling = false
        destinationField.text = root.ensureExtension(destinationField.text.trim())
        root.volumesModel.exportVolume(root.volumeName,
                                       destinationField.text,
                                       String(formatBox.currentValue))
    }

    function cancelOrClose() {
        if (root.exporting) {
            root.cancelling = true
            root.volumesModel.cancelExport()
        } else {
            root.close()
        }
    }

    ListModel { id: formatModel }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        QQC2.Label {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Export volume “%1” through a restricted, read-only helper container.", root.volumeName)
            wrapMode: Text.WrapAnywhere
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Archive format")
            font.bold: true
        }

        QQC2.ComboBox {
            id: formatBox
            Layout.fillWidth: true
            enabled: !root.exporting
            model: formatModel
            textRole: "label"
            valueRole: "value"
            Accessible.name: I18n.i18nd("tuxstack", "Archive format")
            onActivated: {
                const path = destinationField.text
                              .replace(/\.tar\.gz$/i, "")
                              .replace(/\.tar\.zst$/i, "")
                              .replace(/\.tar$/i, "")
                destinationField.text = path + root.extension()
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Destination")
            font.bold: true
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.TextField {
                id: destinationField
                Layout.fillWidth: true
                enabled: !root.exporting
                selectByMouse: true
                font.family: "monospace"
                Accessible.name: I18n.i18nd("tuxstack", "Export destination")
                onAccepted: root.beginExport()
            }
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "Browse…")
                icon.name: "document-open-folder"
                enabled: !root.exporting
                onClicked: {
                    saveDialog.selectedFile = destinationField.text
                    saveDialog.open()
                }
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Information
            text: I18n.i18nd("tuxstack", "The source volume is mounted read-only. The temporary helper container has no Docker socket, privileged access, or network access.")
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: root.exporting || root.cancelling
                     || (root.submitted && root.volumesModel
                         && root.volumesModel.exportStatus.length > 0)
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                Layout.fillWidth: true
                text: root.cancelling
                      ? I18n.i18nd("tuxstack", "Cancelling export…")
                      : (root.volumesModel ? root.volumesModel.exportStatus : "")
                wrapMode: Text.Wrap
            }
            QQC2.ProgressBar {
                Layout.fillWidth: true
                indeterminate: root.exporting
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.submitted && root.volumesModel
                     && root.volumesModel.exportErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.volumesModel.exportErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.exporting
                  ? I18n.i18nd("tuxstack", "Cancel Export")
                  : I18n.i18nd("tuxstack", "Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.cancelOrClose()
        }
        QQC2.Button {
            text: root.exporting
                  ? I18n.i18nd("tuxstack", "Exporting…")
                  : I18n.i18nd("tuxstack", "Export")
            icon.name: "document-export"
            enabled: root.volumesModel && !root.exporting
                     && destinationField.text.trim().length > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.beginExport()
        }
    }

    Dialogs.FileDialog {
        id: saveDialog
        title: I18n.i18nd("tuxstack", "Choose Volume Export Destination")
        fileMode: Dialogs.FileDialog.SaveFile
        nameFilters: formatBox.currentValue === "tar_gzip"
                     ? [I18n.i18nd("tuxstack", "Gzip-compressed tar archives (*.tar.gz)")]
                     : (formatBox.currentValue === "tar_zstd"
                        ? [I18n.i18nd("tuxstack", "Zstandard-compressed tar archives (*.tar.zst)")]
                        : [I18n.i18nd("tuxstack", "Tar archives (*.tar)")])
        acceptLabel: I18n.i18nd("tuxstack", "Select")
        onAccepted: {
            const value = String(selectedFile)
            destinationField.text = value.indexOf("file://") === 0
                                    ? decodeURIComponent(value.substring(7)) : value
        }
    }
}
