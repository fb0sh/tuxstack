pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

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
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        visible: String(containerDelegate.model.status).length > 0
                        text: String(containerDelegate.model.status)
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }
                }

                QQC2.Label {
                    text: String(containerDelegate.model.state)
                    color: String(containerDelegate.model.state) === "running"
                           ? Kirigami.Theme.positiveTextColor
                           : Kirigami.Theme.disabledTextColor
                }

                Kirigami.Icon {
                    source: "go-next"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                }
            }

                background: Rectangle {
                    color: containerRow.hovered
                           ? Qt.rgba(Kirigami.Theme.highlightColor.r,
                                     Kirigami.Theme.highlightColor.g,
                                     Kirigami.Theme.highlightColor.b, 0.12)
                           : "transparent"
                }
            }
        }
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: usageRepeater.count === 0
        text: qsTr("No containers are using this image.")
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }
}
