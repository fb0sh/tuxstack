import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.ItemDelegate {
    id: root

    property string volumeName: ""
    property string displayName: ""
    property string driver: ""
    property string sizeText: ""
    property int usedByCount: 0
    property bool inUse: false
    property bool anonymous: false
    property bool selected: false
    property bool busy: false
    property string operation: ""

    signal selectedRequested(string volumeName)
    signal removeRequested(string volumeName)

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
    Accessible.name: root.displayName.length > 0 ? root.displayName : root.volumeName
    Accessible.description: root.inUse
                            ? (root.usedByCount === 1
                               ? I18n.i18nd("tuxstack", "%1, used by 1 container", root.sizeText)
                               : I18n.i18nd("tuxstack", "%1, used by %2 containers", root.sizeText, root.usedByCount))
                            : I18n.i18nd("tuxstack", "%1, unused", root.sizeText)
    onClicked: root.selectedRequested(root.volumeName)

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
            source: "drive-harddisk"
            active: root.enabled && (root.hovered || root.down)
            selected: root.enabled && root.selected
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing / 2

            QQC2.Label {
                id: nameLabel
                Layout.fillWidth: true
                text: root.displayName.length > 0 ? root.displayName : root.volumeName
                color: root.selected ? Kirigami.Theme.highlightedTextColor
                                     : Kirigami.Theme.textColor
                font.bold: root.selected
                elide: Text.ElideMiddle

                HoverHandler { id: nameHover }
                QQC2.ToolTip.visible: nameHover.hovered
                                      && root.volumeName !== nameLabel.text
                QQC2.ToolTip.text: root.volumeName
                QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: {
                    const size = root.sizeText.length > 0
                                 ? root.sizeText : I18n.i18nd("tuxstack", "Unknown size")
                    if (root.inUse) {
                        return root.usedByCount === 1
                               ? I18n.i18nd("tuxstack", "%1 · 1 container", size)
                               : I18n.i18nd("tuxstack", "%1 · %2 containers", size, root.usedByCount)
                    }
                    return I18n.i18nd("tuxstack", "%1 · Unused", size)
                }
                color: root.selected ? Kirigami.Theme.highlightedTextColor
                                     : Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: root.anonymous
                text: I18n.i18nd("tuxstack", "Anonymous volume")
                color: root.selected ? Kirigami.Theme.highlightedTextColor
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
            Accessible.name: root.operation.length > 0
                             ? root.operation
                             : I18n.i18nd("tuxstack", "Volume operation in progress")
        }

        QQC2.ToolButton {
            id: removeButton
            visible: !root.busy
            opacity: root.hovered || activeFocus || root.visualFocus ? 1 : 0.55
            icon.name: "edit-delete"
            text: I18n.i18nd("tuxstack", "Remove volume “%1”", root.volumeName)
            display: QQC2.AbstractButton.IconOnly
            focusPolicy: Qt.StrongFocus
            onClicked: root.removeRequested(root.volumeName)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
        }
    }
}
