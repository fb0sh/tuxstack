import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components"

Kirigami.Dialog {
    id: root

    property var volumesModel: null
    property string sourceVolume: ""
    property bool submitted: false
    property bool cancelling: false
    readonly property bool cloning: root.volumesModel
                                    && root.volumesModel.cloningSourceName === root.sourceVolume
    readonly property bool valid: targetName.text.trim().length > 0
                                  && driverField.text.trim().length > 0
                                  && optionsEditor.validationError.length === 0

    title: I18n.i18nd("tuxstack", "Clone Volume")
    preferredWidth: Kirigami.Units.gridUnit * 32
    closePolicy: root.cloning ? QQC2.Popup.NoAutoClose
                              : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function suggestedName(name) {
        return String(name) + "-clone"
    }

    function prepare(name) {
        root.sourceVolume = String(name)
        root.submitted = false
        root.cancelling = false
        targetName.text = root.suggestedName(root.sourceVolume)
        driverField.text = root.volumesModel
                           && root.volumesModel.detailDriver.length > 0
                           ? root.volumesModel.detailDriver : "local"
        optionsEditor.clear()
        copyLabels.checked = true
        cleanupFailed.checked = true
        open()
        targetName.selectAll()
        targetName.forceActiveFocus()
    }

    function beginClone() {
        if (!root.volumesModel || !root.valid || root.cloning)
            return
        root.submitted = true
        root.cancelling = false
        root.volumesModel.cloneVolume(root.sourceVolume,
                                      targetName.text.trim(),
                                      driverField.text.trim(),
                                      optionsEditor.entries(),
                                      copyLabels.checked,
                                      cleanupFailed.checked)
    }

    function cancelOrClose() {
        if (root.cloning) {
            root.cancelling = true
            root.volumesModel.cancelClone()
        } else {
            root.close()
        }
    }

    QQC2.ScrollView {
        implicitWidth: Kirigami.Units.gridUnit * 30
        implicitHeight: Math.min(contentColumn.implicitHeight,
                                 Kirigami.Units.gridUnit * 30)
        contentWidth: availableWidth
        QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

        ColumnLayout {
            id: contentColumn
            width: parent.width
            spacing: Kirigami.Units.mediumSpacing

            PropertyList {
                Layout.fillWidth: true
                PropertyRow {
                    Layout.fillWidth: true
                    label: I18n.i18nd("tuxstack", "Source volume")
                    value: root.sourceVolume
                    copyable: true
                    toolTipText: root.sourceVolume
                }
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    text: I18n.i18nd("tuxstack", "Target name")
                    Layout.alignment: Qt.AlignTop
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    QQC2.TextField {
                        id: targetName
                        Layout.fillWidth: true
                        enabled: !root.cloning
                        selectByMouse: true
                        placeholderText: I18n.i18nd("tuxstack", "Required target volume name")
                        Accessible.name: I18n.i18nd("tuxstack", "Target volume name")
                        onAccepted: root.beginClone()
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        visible: targetName.text.length > 0
                                 && targetName.text.trim().length === 0
                        text: I18n.i18nd("tuxstack", "Target name is required.")
                        color: Kirigami.Theme.negativeTextColor
                        font: Kirigami.Theme.smallFont
                    }
                }

                QQC2.Label {
                    text: I18n.i18nd("tuxstack", "Target driver")
                    Layout.alignment: Qt.AlignTop
                }
                QQC2.TextField {
                    id: driverField
                    Layout.fillWidth: true
                    enabled: !root.cloning
                    selectByMouse: true
                    placeholderText: I18n.i18nd("tuxstack", "local")
                    Accessible.name: I18n.i18nd("tuxstack", "Target volume driver")
                    onAccepted: root.beginClone()
                }
            }

            VolumeKeyValueEditor {
                id: optionsEditor
                Layout.fillWidth: true
                title: I18n.i18nd("tuxstack", "Target Driver Options")
                addText: I18n.i18nd("tuxstack", "Add driver option")
                keyPlaceholder: I18n.i18nd("tuxstack", "Option key")
                valuePlaceholder: I18n.i18nd("tuxstack", "Option value")
                editable: !root.cloning
            }

            QQC2.CheckBox {
                id: copyLabels
                text: I18n.i18nd("tuxstack", "Copy source volume labels")
                enabled: !root.cloning
            }

            QQC2.CheckBox {
                id: cleanupFailed
                text: I18n.i18nd("tuxstack", "Remove an incomplete target volume if cloning fails")
                enabled: !root.cloning
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                type: Kirigami.MessageType.Information
                text: I18n.i18nd("tuxstack", "The source is mounted read-only. Hidden files, symbolic links, ownership, permissions, and timestamps are preserved where Docker and the target driver allow it. An existing target volume is never overwritten.")
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: root.cloning || root.cancelling
                         || (root.submitted && root.volumesModel
                             && root.volumesModel.cloneStatus.length > 0)
                spacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    Layout.fillWidth: true
                    text: root.cancelling
                          ? I18n.i18nd("tuxstack", "Cancelling clone…")
                          : (root.volumesModel ? root.volumesModel.cloneStatus : "")
                    wrapMode: Text.Wrap
                }
                QQC2.ProgressBar {
                    Layout.fillWidth: true
                    indeterminate: root.cloning
                }
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: root.submitted && root.volumesModel
                         && root.volumesModel.cloneErrorMessage.length > 0
                type: Kirigami.MessageType.Error
                text: visible ? root.volumesModel.cloneErrorMessage : ""
            }
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.cloning
                  ? I18n.i18nd("tuxstack", "Cancel Clone")
                  : I18n.i18nd("tuxstack", "Close")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.cancelOrClose()
        }
        QQC2.Button {
            text: root.cloning
                  ? I18n.i18nd("tuxstack", "Cloning…")
                  : I18n.i18nd("tuxstack", "Clone")
            icon.name: "edit-copy"
            enabled: root.volumesModel && !root.cloning && root.valid
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.beginClone()
        }
    }
}
