import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components"

Kirigami.Dialog {
    id: root

    property var volumesModel: null
    property bool submitted: false
    readonly property bool creating: root.volumesModel && root.volumesModel.creating
    readonly property bool valid: driverField.text.trim().length > 0
                                  && optionsEditor.validationError.length === 0
                                  && labelsEditor.validationError.length === 0

    title: I18n.i18nd("tuxstack", "Create Volume")
    preferredWidth: Kirigami.Units.gridUnit * 32
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing
    closePolicy: root.creating ? QQC2.Popup.NoAutoClose
                               : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function prepare() {
        root.submitted = false
        nameField.clear()
        driverField.text = "local"
        optionsEditor.clear()
        labelsEditor.clear()
        open()
        nameField.forceActiveFocus()
    }

    function beginCreate() {
        if (!root.volumesModel || !root.valid || root.creating)
            return
        root.submitted = true
        root.volumesModel.createVolume(nameField.text.trim(),
                                       driverField.text.trim(),
                                       optionsEditor.entries(),
                                       labelsEditor.entries())
    }

    function cancelOrClose() {
        if (root.creating)
            cancelDialog.open()
        else
            root.close()
    }

    QQC2.ScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        implicitWidth: Kirigami.Units.gridUnit * 30
        implicitHeight: Math.min(contentColumn.implicitHeight,
                                 Kirigami.Units.gridUnit * 32)
        contentWidth: availableWidth
        QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

        ColumnLayout {
            id: contentColumn
            width: parent.width
            spacing: Kirigami.Units.mediumSpacing

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    text: I18n.i18nd("tuxstack", "Name")
                    Layout.alignment: Qt.AlignTop
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    QQC2.TextField {
                        id: nameField
                        Layout.fillWidth: true
                        enabled: !root.creating
                        selectByMouse: true
                        placeholderText: I18n.i18nd("tuxstack", "Leave blank for a Docker-generated name")
                        Accessible.description: I18n.i18nd("tuxstack", "Optional volume name")
                        onAccepted: root.beginCreate()
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: I18n.i18nd("tuxstack", "Docker generates the name when this field is empty.")
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                        wrapMode: Text.Wrap
                    }
                }

                QQC2.Label {
                    text: I18n.i18nd("tuxstack", "Driver")
                    Layout.alignment: Qt.AlignTop
                }
                QQC2.TextField {
                    id: driverField
                    Layout.fillWidth: true
                    enabled: !root.creating
                    selectByMouse: true
                    text: "local"
                    placeholderText: I18n.i18nd("tuxstack", "local")
                    Accessible.description: I18n.i18nd("tuxstack", "Docker volume driver")
                    onAccepted: root.beginCreate()
                }
            }

            VolumeKeyValueEditor {
                id: optionsEditor
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Driver Options")
                addText: I18n.i18nd("tuxstack", "Add driver option")
                keyPlaceholder: I18n.i18nd("tuxstack", "Option key")
                valuePlaceholder: I18n.i18nd("tuxstack", "Option value")
                editable: !root.creating
            }

            VolumeKeyValueEditor {
                id: labelsEditor
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Labels")
                addText: I18n.i18nd("tuxstack", "Add label")
                keyPlaceholder: I18n.i18nd("tuxstack", "Label key")
                valuePlaceholder: I18n.i18nd("tuxstack", "Label value")
                editable: !root.creating
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: root.submitted && root.volumesModel
                         && root.volumesModel.createErrorMessage.length > 0
                type: Kirigami.MessageType.Error
                text: visible ? root.volumesModel.createErrorMessage : ""
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.creating
                  ? I18n.i18nd("tuxstack", "Cancel Creation…")
                  : I18n.i18nd("tuxstack", "Cancel")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.cancelOrClose()
        }
        QQC2.Button {
            text: root.creating
                  ? I18n.i18nd("tuxstack", "Creating…")
                  : I18n.i18nd("tuxstack", "Create")
            icon.name: "list-add"
            enabled: root.volumesModel && !root.creating && root.valid
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.beginCreate()
        }
    }

    Kirigami.Dialog {
        id: cancelDialog
        title: I18n.i18nd("tuxstack", "Cancel Volume Creation?")
        preferredWidth: Kirigami.Units.gridUnit * 24
        leftPadding: Kirigami.Units.largeSpacing
        rightPadding: Kirigami.Units.largeSpacing
        topPadding: Kirigami.Units.largeSpacing
        bottomPadding: Kirigami.Units.largeSpacing

        QQC2.Label {
            width: parent.width
            text: I18n.i18nd("tuxstack", "The creation request will be cancelled if possible. Your entries will remain available until this dialog is closed.")
            wrapMode: Text.Wrap
        }

        footer: QQC2.DialogButtonBox {
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "Continue Creating")
                QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
                onClicked: cancelDialog.close()
            }
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "Cancel Creation")
                icon.name: "dialog-cancel"
                QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
                onClicked: {
                    cancelDialog.close()
                    if (root.volumesModel)
                        root.volumesModel.cancelCreate()
                }
            }
        }
    }
}
