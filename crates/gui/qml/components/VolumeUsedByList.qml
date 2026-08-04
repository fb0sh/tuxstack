pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

ColumnLayout {
    id: root

    property var sourceModel: null
    signal containerRequested(string containerId)

    spacing: 0

    Repeater {
        id: usageRepeater
        model: root.sourceModel

        delegate: Item {
            id: containerDelegate
            required property var model

            Layout.fillWidth: true
            implicitHeight: containerRow.implicitHeight

            QQC2.ItemDelegate {
                id: containerRow
                width: parent.width
                hoverEnabled: true
                focusPolicy: Qt.StrongFocus
                Accessible.name: I18n.i18nd("tuxstack", "Open container %1").arg(String(containerDelegate.model.name))
                Accessible.description: I18n.i18nd("tuxstack", "%1 mounted at %2")
                                        .arg(String(containerDelegate.model.state))
                                        .arg(String(containerDelegate.model.destination))
                onClicked: root.containerRequested(String(containerDelegate.model.containerId))

                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing

                    Kirigami.Icon {
                        source: "system-run"
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        QQC2.Label {
                            Layout.fillWidth: true
                            text: String(containerDelegate.model.name)
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            text: String(containerDelegate.model.shortId)
                            color: Kirigami.Theme.disabledTextColor
                            font.family: "monospace"
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            elide: Text.ElideMiddle
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            text: String(containerDelegate.model.destination)
                            color: Kirigami.Theme.disabledTextColor
                            font.family: "monospace"
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            elide: Text.ElideMiddle
                            QQC2.ToolTip.visible: destinationHover.hovered
                            QQC2.ToolTip.text: text
                            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                            HoverHandler { id: destinationHover }
                        }
                    }

                    ColumnLayout {
                        spacing: 0
                        QQC2.Label {
                            Layout.alignment: Qt.AlignRight
                            text: String(containerDelegate.model.state)
                            color: String(containerDelegate.model.state).toLowerCase() === "running"
                                   ? Kirigami.Theme.positiveTextColor
                                   : Kirigami.Theme.disabledTextColor
                        }
                        QQC2.Label {
                            Layout.alignment: Qt.AlignRight
                            text: Boolean(containerDelegate.model.readOnly)
                                  ? I18n.i18nd("tuxstack", "Read Only")
                                  : I18n.i18nd("tuxstack", "Read/Write")
                            color: Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                        }
                    }

                    Kirigami.Icon {
                        source: "go-next"
                        Layout.preferredWidth: Kirigami.Units.iconSizes.small
                        Layout.preferredHeight: Kirigami.Units.iconSizes.small
                    }
                }

                background: Rectangle {
                    color: containerRow.hovered || containerRow.visualFocus
                           ? Qt.alpha(Kirigami.Theme.highlightColor, 0.12)
                           : "transparent"
                    border.width: containerRow.visualFocus ? 1 : 0
                    border.color: Kirigami.Theme.highlightColor
                }
            }
        }
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: usageRepeater.count === 0
        text: I18n.i18nd("tuxstack", "No containers are using this volume.")
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }
}
