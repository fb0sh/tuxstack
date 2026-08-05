pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var volumesModel: null
    property bool submitted: false
    readonly property bool pruning: root.volumesModel && root.volumesModel.pruning
    readonly property int candidateCount: candidateList.count

    title: I18n.i18nd("tuxstack", "Remove Unused Volumes")
    preferredWidth: Kirigami.Units.gridUnit * 32
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    topPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing
    closePolicy: root.pruning ? QQC2.Popup.NoAutoClose
                              : QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside

    function prepare() {
        root.submitted = false
        open()
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Heading {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Remove unused volumes?")
            level: 3
            wrapMode: Text.Wrap
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: root.candidateCount === 1
                  ? I18n.i18nd("tuxstack", "The following volume is not referenced by any existing container:")
                  : I18n.i18nd("tuxstack", "The following %1 volumes are not referenced by any existing container:", root.candidateCount)
            wrapMode: Text.Wrap
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(candidateList.contentHeight,
                                             Kirigami.Units.gridUnit * 14)
            Layout.minimumHeight: Math.min(candidateList.contentHeight,
                                           Kirigami.Units.gridUnit * 4)
            color: Kirigami.Theme.alternateBackgroundColor

            ListView {
                id: candidateList
                anchors.fill: parent
                clip: true
                model: root.volumesModel ? root.volumesModel.pruneCandidateModel : null
                activeFocusOnTab: true
                Accessible.name: I18n.i18nd("tuxstack", "Unused volumes to remove")

                delegate: RowLayout {
                    id: candidateRow
                    required property string volumeName
                    required property string sizeText
                    width: candidateList.width
                    height: Math.max(nameLabel.implicitHeight, sizeLabel.implicitHeight)
                            + Kirigami.Units.mediumSpacing * 2
                    spacing: Kirigami.Units.mediumSpacing

                    QQC2.Label {
                        id: nameLabel
                        Layout.leftMargin: Kirigami.Units.mediumSpacing
                        Layout.fillWidth: true
                        text: candidateRow.volumeName
                        font.family: "monospace"
                        elide: Text.ElideMiddle
                        QQC2.ToolTip.visible: nameHover.hovered
                        QQC2.ToolTip.text: candidateRow.volumeName
                        HoverHandler { id: nameHover }
                    }
                    QQC2.Label {
                        id: sizeLabel
                        Layout.rightMargin: Kirigami.Units.mediumSpacing
                        text: candidateRow.sizeText.length > 0
                              ? candidateRow.sizeText
                              : I18n.i18nd("tuxstack", "Unknown")
                        color: Kirigami.Theme.disabledTextColor
                    }
                }

                QQC2.ScrollBar.vertical: QQC2.ScrollBar { }
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: {
                if (!root.volumesModel)
                    return I18n.i18nd("tuxstack", "Reclaimable size unavailable")
                const known = String(root.volumesModel.pruneKnownSizeText || "0 B")
                const unknown = Number(root.volumesModel.pruneUnknownSizeCount || 0)
                if (root.candidateCount > 0 && unknown === root.candidateCount)
                    return I18n.i18nd("tuxstack", "Reclaimable size unavailable · all volume sizes are unknown")
                if (unknown === 1)
                    return I18n.i18nd("tuxstack", "Known reclaimable size: %1 · 1 volume has unknown size", known)
                if (unknown > 1)
                    return I18n.i18nd("tuxstack", "Known reclaimable size: %1 · %2 volumes have unknown size", known, unknown)
                return I18n.i18nd("tuxstack", "Known reclaimable size: %1", known)
            }
            font.bold: true
            wrapMode: Text.Wrap
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Warning
            text: I18n.i18nd("tuxstack", "Docker Volume Prune permanently deletes these unreferenced volumes and their data. TuxStack will not remove containers. This action cannot be undone.")
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.submitted && root.volumesModel
                     && root.volumesModel.pruneErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.volumesModel.pruneErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: root.pruning
                  ? I18n.i18nd("tuxstack", "Cancel Prune")
                  : I18n.i18nd("tuxstack", "Cancel")
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: {
                if (root.pruning)
                    root.volumesModel.cancelPrune()
                else
                    root.close()
            }
        }
        QQC2.Button {
            text: root.pruning
                  ? I18n.i18nd("tuxstack", "Removing Volumes…")
                  : I18n.i18nd("tuxstack", "Remove Volumes")
            icon.name: "edit-clear-history"
            enabled: root.volumesModel && !root.pruning && root.candidateCount > 0
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: {
                root.submitted = true
                root.volumesModel.pruneVolumes()
            }
        }
    }
}
