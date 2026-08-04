import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * A theme-aware, keyboard accessible navigation item.
 */
QQC2.ItemDelegate {
    id: root

    property string iconName: "application-x-executable"
    property string pageId: ""
    property bool selected: false
    property bool collapsed: false

    signal triggered(string pageId)

    readonly property color hoverBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.14)
    readonly property color pressedBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.24)
    readonly property color focusBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.10)
    readonly property color focusOutlineColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.65)

    hoverEnabled: true
    checkable: true
    checked: selected
    focusPolicy: Qt.StrongFocus
    implicitHeight: Kirigami.Units.gridUnit * 2.25
    leftPadding: Kirigami.Units.mediumSpacing
    rightPadding: Kirigami.Units.mediumSpacing
    topPadding: 0
    bottomPadding: 0

    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.description: selected ? qsTr("Current page") : qsTr("Open page")

    onClicked: triggered(pageId)

    Keys.onUpPressed: function(event) {
        nextItemInFocusChain(false).forceActiveFocus(Qt.BacktabFocusReason)
        event.accepted = true
    }
    Keys.onDownPressed: function(event) {
        nextItemInFocusChain(true).forceActiveFocus(Qt.TabFocusReason)
        event.accepted = true
    }

    background: Item {
        Rectangle {
            anchors.fill: buttonBackground
            anchors.margins: -2
            radius: Kirigami.Units.smallSpacing + 2
            color: "transparent"
            border.width: 2
            border.color: root.enabled && root.visualFocus
                ? root.focusOutlineColor
                : "transparent"

            Behavior on border.color {
                ColorAnimation { duration: Kirigami.Units.shortDuration }
            }
        }

        Rectangle {
            id: buttonBackground

            anchors.fill: parent
            radius: Kirigami.Units.smallSpacing
            color: {
                if (!root.enabled)
                    return "transparent"
                if (root.selected)
                    return Kirigami.Theme.highlightColor
                if (root.down)
                    return root.pressedBackgroundColor
                if (root.hovered)
                    return root.hoverBackgroundColor
                if (root.visualFocus)
                    return root.focusBackgroundColor
                return "transparent"
            }
            border.width: 1
            border.color: {
                if (!root.enabled || root.selected)
                    return "transparent"
                if (root.down || root.hovered)
                    return Kirigami.Theme.highlightColor
                return "transparent"
            }

            Behavior on color {
                ColorAnimation { duration: Kirigami.Units.shortDuration }
            }
            Behavior on border.color {
                ColorAnimation { duration: Kirigami.Units.shortDuration }
            }
        }
    }

    contentItem: RowLayout {
        spacing: root.collapsed ? 0 : Kirigami.Units.mediumSpacing

        Kirigami.Icon {
            source: root.iconName
            active: root.enabled && (root.hovered || root.down)
            selected: root.enabled && root.selected
            Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
            Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            Layout.alignment: Qt.AlignCenter
            Accessible.ignored: true
        }

        QQC2.Label {
            text: root.text
            visible: !root.collapsed
            opacity: visible ? 1 : 0
            color: {
                if (!root.enabled)
                    return Kirigami.Theme.disabledTextColor
                if (root.selected)
                    return Kirigami.Theme.highlightedTextColor
                return Kirigami.Theme.textColor
            }
            elide: Text.ElideRight
            verticalAlignment: Text.AlignVCenter
            Layout.fillWidth: true

            Behavior on opacity {
                NumberAnimation { duration: Kirigami.Units.shortDuration }
            }
        }
    }

    QQC2.ToolTip.text: text
    QQC2.ToolTip.visible: enabled && hovered && collapsed
    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
}
