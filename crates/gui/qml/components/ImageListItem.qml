import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

QQC2.ItemDelegate {
    id: root

    property string imageId: ""
    property string displayName: ""
    property string secondaryText: ""
    property string architecture: ""
    property string iconName: "package-x-generic"
    property bool selected: false
    property bool inUse: false
    property bool busy: false
    property int usedByCount: 0
    property int additionalTagCount: 0

    signal selectedRequested(string imageId)
    signal removeRequested(string imageId)

    readonly property color hoverBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.14)
    readonly property color pressedBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.24)
    readonly property color focusBackgroundColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.10)
    readonly property color focusOutlineColor: Qt.alpha(Kirigami.Theme.highlightColor, 0.65)

    width: ListView.view ? ListView.view.width : implicitWidth
    implicitHeight: Kirigami.Units.gridUnit * 4
    hoverEnabled: true
    checkable: true
    checked: selected
    focusPolicy: Qt.StrongFocus
    leftPadding: Kirigami.Units.mediumSpacing
    rightPadding: Kirigami.Units.smallSpacing
    topPadding: Kirigami.Units.mediumSpacing
    bottomPadding: Kirigami.Units.mediumSpacing
    Accessible.name: displayName
    Accessible.description: secondaryText
    onClicked: selectedRequested(imageId)

    background: Item {
        Rectangle {
            anchors.fill: buttonBackground
            anchors.margins: -2
            radius: Kirigami.Units.smallSpacing + 2
            color: "transparent"
            border.width: 2
            border.color: root.enabled && root.visualFocus
                          ? root.focusOutlineColor : "transparent"

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
            border.color: root.enabled && !root.selected
                          && (root.down || root.hovered)
                          ? Kirigami.Theme.highlightColor : "transparent"

            Behavior on color {
                ColorAnimation { duration: Kirigami.Units.shortDuration }
            }
            Behavior on border.color {
                ColorAnimation { duration: Kirigami.Units.shortDuration }
            }
        }
    }

    contentItem: RowLayout {
        spacing: Kirigami.Units.mediumSpacing

        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            Layout.alignment: Qt.AlignVCenter
            source: root.iconName.length > 0 ? root.iconName : "package-x-generic"
            active: root.enabled && (root.hovered || root.down)
            selected: root.enabled && root.selected
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing / 2

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    Layout.fillWidth: true
                    text: root.displayName.length > 0 ? root.displayName : qsTr("<none>:<none>")
                    color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                    font.bold: root.selected
                    elide: Text.ElideRight
                }

                QQC2.Label {
                    visible: root.additionalTagCount > 0
                    text: qsTr("+%1").arg(root.additionalTagCount)
                    color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    Layout.fillWidth: true
                    text: root.secondaryText
                    color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                    font: Kirigami.Theme.smallFont
                    elide: Text.ElideRight
                }

                Rectangle {
                    visible: root.architecture.length > 0
                    implicitWidth: architectureLabel.implicitWidth + Kirigami.Units.mediumSpacing
                    implicitHeight: architectureLabel.implicitHeight + Kirigami.Units.smallSpacing
                    radius: Kirigami.Units.smallSpacing
                    color: root.selected
                           ? Qt.rgba(Kirigami.Theme.highlightedTextColor.r,
                                     Kirigami.Theme.highlightedTextColor.g,
                                     Kirigami.Theme.highlightedTextColor.b, 0.18)
                           : Kirigami.Theme.alternateBackgroundColor

                    QQC2.Label {
                        id: architectureLabel
                        anchors.centerIn: parent
                        text: root.architecture
                        color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                        font: Kirigami.Theme.smallFont
                    }
                }
            }
        }

        QQC2.BusyIndicator {
            visible: root.busy
            running: visible
            Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
            Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
        }

        QQC2.ToolButton {
            visible: !root.busy
            icon.name: "edit-delete"
            text: root.inUse
                  ? qsTr("Remove image used by %1 container(s)").arg(root.usedByCount)
                  : qsTr("Remove image")
            display: QQC2.AbstractButton.IconOnly
            onClicked: root.removeRequested(root.imageId)
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: text
            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
        }
    }
}
