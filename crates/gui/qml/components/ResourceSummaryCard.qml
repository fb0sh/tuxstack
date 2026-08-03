import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Overview summary card: icon, label, value.
 */
Kirigami.AbstractCard {
    id: root

    property string iconName: ""
    property string label: ""
    property string value: ""

    contentItem: RowLayout {
        spacing: Kirigami.Units.mediumSpacing
        anchors.margins: Kirigami.Units.largeSpacing

        Kirigami.Icon {
            source: root.iconName
            implicitWidth: Kirigami.Units.iconSizes.medium
            implicitHeight: Kirigami.Units.iconSizes.medium
            color: Kirigami.Theme.textColor
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            QQC2.Label {
                text: root.value
                font.bold: true
                Layout.fillWidth: true
                elide: Text.ElideRight
            }
            QQC2.Label {
                text: root.label
                color: Kirigami.Theme.disabledTextColor
                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                Layout.fillWidth: true
                elide: Text.ElideRight
            }
        }
    }
}
