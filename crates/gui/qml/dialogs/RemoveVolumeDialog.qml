import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components"

Kirigami.Dialog {
    id: root

    property var volumesModel: null
    property string volumeName: ""
    property string driver: ""
    property string sizeText: ""
    property int usedByCount: 0
    property string mountpoint: ""
    property bool submitted: false
    readonly property bool removing: root.volumesModel
                                     && root.volumesModel.removingVolumeName === root.volumeName

    title: I18n.i18nd("tuxstack", "Remove Volume")
    preferredWidth: Kirigami.Units.gridUnit * 30
    closePolicy: root.removing ? QQC2.Popup.NoAutoClose
                               : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function prepare(name, volumeDriver, knownSize, containerCount, path) {
        root.volumeName = String(name)
        root.driver = String(volumeDriver || "")
        root.sizeText = String(knownSize || "")
        root.usedByCount = Number(containerCount || 0)
        root.mountpoint = String(path || "")
        root.submitted = false
        open()
    }

    ColumnLayout {
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Remove volume “%1”?").arg(root.volumeName)
            level: 3
            wrapMode: Text.WrapAnywhere
        }

        PropertyList {
            Layout.fillWidth: true
            PropertyRow {
                Layout.fillWidth: true
                label: I18n.i18nd("tuxstack", "Name")
                value: root.volumeName
                copyable: true
                toolTipText: root.volumeName
            }
            PropertyRow {
                Layout.fillWidth: true
                label: I18n.i18nd("tuxstack", "Driver")
                value: root.driver.length > 0 ? root.driver
                                              : I18n.i18nd("tuxstack", "—")
            }
            PropertyRow {
                Layout.fillWidth: true
                label: I18n.i18nd("tuxstack", "Known size")
                value: root.sizeText.length > 0 ? root.sizeText
                                                : I18n.i18nd("tuxstack", "Unknown")
            }
            PropertyRow {
                Layout.fillWidth: true
                label: I18n.i18nd("tuxstack", "Used by")
                value: root.usedByCount === 1
                       ? I18n.i18nd("tuxstack", "1 container")
                       : I18n.i18nd("tuxstack", "%1 containers").arg(root.usedByCount)
            }
            PropertyRow {
                Layout.fillWidth: true
                label: I18n.i18nd("tuxstack", "Mountpoint")
                value: root.mountpoint.length > 0 ? root.mountpoint
                                                  : I18n.i18nd("tuxstack", "—")
                copyable: root.mountpoint.length > 0
                monospace: true
                toolTipText: root.mountpoint
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.usedByCount > 0
            type: Kirigami.MessageType.Warning
            text: I18n.i18nd("tuxstack", "This volume is referenced by one or more containers. TuxStack will not stop, remove, or modify those containers. Docker will reject removal while the volume is in use.")
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.usedByCount === 0
            type: Kirigami.MessageType.Warning
            text: I18n.i18nd("tuxstack", "Removing this volume permanently deletes all data stored in it. This action cannot be undone.")
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.submitted && root.volumesModel
                     && root.volumesModel.removeErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.volumesModel.removeErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Cancel")
            enabled: !root.removing
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.close()
        }
        QQC2.Button {
            text: root.removing
                  ? I18n.i18nd("tuxstack", "Removing…")
                  : I18n.i18nd("tuxstack", "Remove Volume")
            icon.name: "edit-delete"
            enabled: root.volumesModel && !root.removing && root.usedByCount === 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: {
                root.submitted = true
                root.volumesModel.removeVolume(root.volumeName, false)
            }
        }
    }
}
