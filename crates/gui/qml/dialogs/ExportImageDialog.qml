import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Dialog {
    id: root

    property var imagesModel: null
    property string destinationPath: ""
    property bool cancelling: false
    readonly property bool exporting: imagesModel && imagesModel.exporting

    title: qsTr("Export Image")
    preferredWidth: Kirigami.Units.gridUnit * 28
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing
    closePolicy: root.exporting ? QQC2.Popup.NoAutoClose
                                : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function showFor(path) {
        destinationPath = path
        cancelling = false
        open()
    }

    function cancelOrClose() {
        if (root.exporting) {
            root.cancelling = true
            root.imagesModel.cancelExport()
        } else {
            root.close()
        }
    }

    onClosed: cancelling = false

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        QQC2.Label {
            Layout.fillWidth: true
            text: root.cancelling ? qsTr("Cancelling export…")
                                  : (root.imagesModel && root.imagesModel.exportStatus.length > 0
                                     ? root.imagesModel.exportStatus
                                     : qsTr("Preparing image export…"))
            wrapMode: Text.WordWrap
        }

        QQC2.ProgressBar {
            Layout.fillWidth: true
            indeterminate: root.exporting
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: qsTr("The TuxStack service writes the archive atomically at the selected destination.")
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.disabledTextColor
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: root.destinationPath
            font.family: "monospace"
            elide: Text.ElideMiddle
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.imagesModel && root.imagesModel.exportErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.imagesModel.exportErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.exporting ? qsTr("Cancel Export") : qsTr("Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.cancelOrClose()
        }
    }
}
