import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Dialog {
    id: root

    property var imagesModel: null
    property bool cancelling: false
    readonly property bool pulling: imagesModel && imagesModel.pulling
    readonly property bool customPlatform: platformBox.currentIndex === 3

    title: qsTr("Pull Image")
    preferredWidth: Kirigami.Units.gridUnit * 28
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    function platformValue() {
        switch (platformBox.currentIndex) {
        case 1: return "linux/amd64"
        case 2: return "linux/arm64"
        case 3: return customPlatformField.text.trim()
        default: return ""
        }
    }

    function beginPull() {
        if (!root.imagesModel || imageReference.text.trim().length === 0)
            return
        root.cancelling = false
        root.imagesModel.pullImage(imageReference.text.trim(), root.platformValue(),
                                   authentication.checked ? username.text : "",
                                   authentication.checked ? password.text : "",
                                   authentication.checked ? registry.text.trim() : "")
        password.clear()
    }

    function cancelOrClose() {
        if (root.pulling) {
            root.cancelling = true
            root.imagesModel.cancelPull()
        } else {
            root.close()
        }
    }

    onAboutToHide: {
        if (root.pulling)
            root.imagesModel.cancelPull()
        password.clear()
    }

    onClosed: cancelling = false

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        QQC2.Label {
            Layout.fillWidth: true
            text: qsTr("Image reference")
            font.bold: true
        }

        QQC2.TextField {
            id: imageReference
            Layout.fillWidth: true
            placeholderText: qsTr("ubuntu:24.04 or registry.example.com/project/image:tag")
            enabled: !root.pulling
            selectByMouse: true
            onAccepted: root.beginPull()
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: qsTr("Platform")
            font.bold: true
        }

        QQC2.ComboBox {
            id: platformBox
            Layout.fillWidth: true
            enabled: !root.pulling
            model: [qsTr("Automatic"), qsTr("linux/amd64"), qsTr("linux/arm64"), qsTr("Custom")]
        }

        QQC2.TextField {
            id: customPlatformField
            Layout.fillWidth: true
            visible: root.customPlatform
            enabled: !root.pulling
            placeholderText: qsTr("linux/architecture[/variant]")
            selectByMouse: true
        }

        QQC2.CheckBox {
            id: authentication
            text: qsTr("Use registry authentication")
            enabled: !root.pulling
        }

        GridLayout {
            Layout.fillWidth: true
            visible: authentication.checked
            columns: 2
            columnSpacing: Kirigami.Units.mediumSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            QQC2.Label { text: qsTr("Registry server") }
            QQC2.TextField {
                id: registry
                Layout.fillWidth: true
                enabled: !root.pulling
                placeholderText: qsTr("registry.example.com")
                selectByMouse: true
            }

            QQC2.Label { text: qsTr("Username") }
            QQC2.TextField {
                id: username
                Layout.fillWidth: true
                enabled: !root.pulling
                selectByMouse: true
            }

            QQC2.Label { text: qsTr("Password / token") }
            QQC2.TextField {
                id: password
                Layout.fillWidth: true
                enabled: !root.pulling
                echoMode: TextInput.Password
                passwordCharacter: "●"
                selectByMouse: true
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.imagesModel && root.imagesModel.pullErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.imagesModel.pullErrorMessage : ""
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: root.pulling || (root.imagesModel && root.imagesModel.pullStatus.length > 0)
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                Layout.fillWidth: true
                text: root.cancelling ? qsTr("Cancelling…")
                                      : (root.imagesModel ? root.imagesModel.pullStatus : "")
                elide: Text.ElideRight
            }

            QQC2.ProgressBar {
                Layout.fillWidth: true
                indeterminate: root.pulling && root.imagesModel && !root.imagesModel.pullProgressKnown
                from: 0
                to: 100
                value: root.imagesModel ? root.imagesModel.pullPercent : 0
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: root.imagesModel && root.imagesModel.pullProgressText.length > 0
                text: visible ? root.imagesModel.pullProgressText : ""
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.pulling ? qsTr("Cancel Pull") : qsTr("Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.cancelOrClose()
        }
        QQC2.Button {
            text: qsTr("Pull")
            icon.name: "download"
            enabled: root.imagesModel && !root.pulling
                     && imageReference.text.trim().length > 0
                     && (!root.customPlatform || customPlatformField.text.trim().length > 0)
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.beginPull()
        }
    }
}
