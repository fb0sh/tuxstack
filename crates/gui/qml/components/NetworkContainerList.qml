pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

ColumnLayout {
    id: root

    property var sourceModel: null

    spacing: 0

    Repeater {
        id: containerRepeater
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
                Accessible.name: String(containerDelegate.model.name)

                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing

                    Kirigami.Icon {
                        source: "container-symbolic"
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
                            text: I18n.i18nd("tuxstack", "Container ID: %1")
                                  .arg(String(containerDelegate.model.shortId))
                            color: Kirigami.Theme.disabledTextColor
                            font.family: "monospace"
                            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            elide: Text.ElideMiddle
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            visible: String(containerDelegate.model.ipv4Address).length > 0
                                     || String(containerDelegate.model.ipv6Address).length > 0
                            text: {
                                const addresses = []
                                const ipv4Address = String(containerDelegate.model.ipv4Address)
                                const ipv6Address = String(containerDelegate.model.ipv6Address)
                                if (ipv4Address.length > 0)
                                    addresses.push(I18n.i18nd("tuxstack", "IPv4: %1")
                                                   .arg(ipv4Address))
                                if (ipv6Address.length > 0)
                                    addresses.push(I18n.i18nd("tuxstack", "IPv6: %1")
                                                   .arg(ipv6Address))
                                return addresses.join("  ·  ")
                            }
                            color: Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                            elide: Text.ElideRight
                        }
                        QQC2.Label {
                            Layout.fillWidth: true
                            visible: String(containerDelegate.model.endpointId).length > 0
                            text: I18n.i18nd("tuxstack", "Endpoint: %1")
                                  .arg(String(containerDelegate.model.endpointId))
                            color: Kirigami.Theme.disabledTextColor
                            font.family: "monospace"
                            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            elide: Text.ElideMiddle
                        }
                    }
                }

                background: Rectangle {
                    color: containerRow.hovered
                           ? Qt.alpha(Kirigami.Theme.highlightColor, 0.12)
                           : "transparent"
                }
            }
        }
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: containerRepeater.count === 0
        text: I18n.i18nd("tuxstack", "No containers attached.")
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }
}
