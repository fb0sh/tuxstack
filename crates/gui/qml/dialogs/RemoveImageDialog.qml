import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Dialog {
    id: root

    property string imageId: ""
    property string displayName: ""
    property string shortId: ""
    property string tagsText: ""
    property string sizeText: ""
    property int usedByCount: 0
    property bool removing: false
    property string errorMessage: ""

    signal removalRequested(string imageId, bool force, bool pruneChildren)

    title: qsTr("Remove Image")
    preferredWidth: Kirigami.Units.gridUnit * 28
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    function prepare(id, name, shortImageId, tags, size, usageCount) {
        imageId = id
        displayName = name
        shortId = shortImageId
        tagsText = tags
        sizeText = size
        usedByCount = usageCount
        errorMessage = ""
        forceCheck.checked = false
        pruneCheck.checked = false
        open()
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            text: qsTr("Remove image “%1”?").arg(root.displayName)
            level: 3
            wrapMode: Text.WordWrap
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            QQC2.Label { text: qsTr("Image ID"); color: Kirigami.Theme.disabledTextColor }
            QQC2.Label { Layout.fillWidth: true; text: root.shortId; font.family: "monospace"; elide: Text.ElideMiddle }

            QQC2.Label { text: qsTr("Tags"); color: Kirigami.Theme.disabledTextColor; Layout.alignment: Qt.AlignTop }
            QQC2.Label { Layout.fillWidth: true; text: root.tagsText.length > 0 ? root.tagsText : qsTr("—"); wrapMode: Text.WrapAnywhere }

            QQC2.Label { text: qsTr("Size"); color: Kirigami.Theme.disabledTextColor }
            QQC2.Label { Layout.fillWidth: true; text: root.sizeText.length > 0 ? root.sizeText : qsTr("—") }

            QQC2.Label { text: qsTr("Used by"); color: Kirigami.Theme.disabledTextColor }
            QQC2.Label { Layout.fillWidth: true; text: qsTr("%1 container(s)").arg(root.usedByCount) }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.errorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: root.errorMessage
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.usedByCount > 0
            type: Kirigami.MessageType.Warning
            text: qsTr("This image is referenced by existing containers. Docker will reject normal removal. Force removal does not delete those containers, and they may stop working correctly.")
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.usedByCount === 0
            type: Kirigami.MessageType.Information
            text: qsTr("Removing an image deletes its tags and layers that are not shared with other images.")
        }

        QQC2.CheckBox {
            id: forceCheck
            text: qsTr("Force removal")
            enabled: !root.removing
        }

        QQC2.CheckBox {
            id: pruneCheck
            text: qsTr("Prune untagged parent images")
            enabled: !root.removing
        }

        QQC2.Label {
            Layout.fillWidth: true
            visible: pruneCheck.checked
            text: qsTr("Untagged parent images that are no longer needed may also be removed.")
            color: Kirigami.Theme.disabledTextColor
            wrapMode: Text.WordWrap
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: qsTr("Cancel")
            enabled: !root.removing
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.close()
        }
        QQC2.Button {
            text: root.removing ? qsTr("Removing…") : qsTr("Remove")
            icon.name: "edit-delete"
            enabled: !root.removing && (root.usedByCount === 0 || forceCheck.checked)
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.removalRequested(root.imageId, forceCheck.checked, pruneCheck.checked)
        }
    }
}
