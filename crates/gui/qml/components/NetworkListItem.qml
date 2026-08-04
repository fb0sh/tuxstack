import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.ItemDelegate {
    id: root

    property string networkId: ""
    property string shortId: ""
    property string name: ""
    property string subnet: ""
    property string gateway: ""
    property string driver: ""
    property string scope: ""
    property bool internal: false
    property bool attachable: false
    property bool ingress: false
    property bool ipv4: false
    property bool ipv6: false
    property bool selected: false
    property bool busy: false
    property string operation: ""

    signal removeRequested()

    readonly property color hoverBackground: Qt.alpha(Kirigami.Theme.highlightColor, 0.14)
    readonly property color pressedBackground: Qt.alpha(Kirigami.Theme.highlightColor, 0.24)
    readonly property color focusBackground: Qt.alpha(Kirigami.Theme.highlightColor, 0.10)

    width: ListView.view ? ListView.view.width : implicitWidth
    implicitHeight: Kirigami.Units.gridUnit * 4
    hoverEnabled: true
    checkable: true
    checked: root.selected
    focusPolicy: Qt.StrongFocus
    leftPadding: Kirigami.Units.mediumSpacing
    rightPadding: Kirigami.Units.smallSpacing
    topPadding: Kirigami.Units.mediumSpacing
    bottomPadding: Kirigami.Units.mediumSpacing
    Accessible.name: root.name
    Accessible.description: root.subnet.length > 0 ? root.subnet : root.driver

    background: Rectangle {
        radius: Kirigami.Units.smallSpacing
        color: {
            if (!root.enabled)
                return "transparent"
            if (root.selected)
                return Kirigami.Theme.highlightColor
            if (root.down)
                return root.pressedBackground
            if (root.hovered)
                return root.hoverBackground
            if (root.visualFocus)
                return root.focusBackground
            return "transparent"
        }
        border.width: root.visualFocus || (root.hovered && !root.selected) ? 1 : 0
        border.color: root.visualFocus || root.hovered
                      ? Kirigami.Theme.highlightColor : "transparent"
    }

    contentItem: RowLayout {
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            Layout.alignment: Qt.AlignVCenter
            source: "network-wired"
            active: root.enabled && (root.hovered || root.down)
            selected: root.enabled && root.selected
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing / 2

            QQC2.Label {
                Layout.fillWidth: true
                text: root.name.length > 0
                      ? root.name : I18n.i18nd("tuxstack", "Unnamed network")
                color: root.selected
                       ? Kirigami.Theme.highlightedTextColor
                       : Kirigami.Theme.textColor
                font.bold: root.selected
                elide: Text.ElideRight
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: root.subnet.length > 0 ? root.subnet : root.driver
                color: root.selected
                       ? Kirigami.Theme.highlightedTextColor
                       : Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
            }
        }

        QQC2.BusyIndicator {
            visible: root.busy
            running: visible
            Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
            Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            Accessible.name: root.operation
        }

        QQC2.ToolButton {
            visible: !root.busy
            icon.name: "edit-delete"
            text: I18n.i18nd("tuxstack", "Remove network")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.removeRequested()
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
        }
    }
}
