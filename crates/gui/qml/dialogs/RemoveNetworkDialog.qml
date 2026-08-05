import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property string networkId: ""
    property string networkName: ""
    property string shortId: ""
    property int containerCount: 0
    property bool removing: false
    property bool submitted: false
    property string errorMessage: ""

    signal removalRequested(string networkId)

    title: I18n.i18nd("tuxstack", "Remove Network")
    preferredWidth: Kirigami.Units.gridUnit * 28
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing
    closePolicy: root.removing ? QQC2.Popup.NoAutoClose
                               : QQC2.Popup.CloseOnEscape

    function prepare(id, name, shortNetworkId, connectedContainers) {
        root.networkId = id
        root.networkName = name
        root.shortId = shortNetworkId
        root.containerCount = Number(connectedContainers)
        root.submitted = false
        open()
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Remove network “%1”?", root.networkName)
            level: 3
            wrapMode: Text.WordWrap
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Network ID")
                color: Kirigami.Theme.disabledTextColor
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: root.shortId
                font.family: "monospace"
                elide: Text.ElideMiddle
            }

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Connected containers")
                color: Kirigami.Theme.disabledTextColor
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: String(root.containerCount)
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.submitted && root.errorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: root.errorMessage
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.containerCount > 0
            type: Kirigami.MessageType.Warning
            text: I18n.i18nd("tuxstack",
                             "This network has connected containers. Docker will reject removal while the network is in use. TuxStack will not disconnect or remove containers automatically.")
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.containerCount === 0
            type: Kirigami.MessageType.Information
            text: I18n.i18nd("tuxstack",
                             "Removing this network permanently deletes its Docker network configuration.")
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
                  : I18n.i18nd("tuxstack", "Remove")
            icon.name: "edit-delete"
            enabled: !root.removing
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: {
                root.submitted = true
                root.removalRequested(root.networkId)
            }
        }
    }
}
