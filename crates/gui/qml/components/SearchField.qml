import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Search field with a clear button.
 */
QQC2.TextField {
    id: root

    property string placeholderTextFallback: I18n.i18nd("tuxstack", "Search…")

    placeholderText: placeholderTextFallback
    implicitWidth: Kirigami.Units.gridUnit * 16
    implicitHeight: Kirigami.Units.gridUnit * 2
    selectByMouse: true

    leftPadding: Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing
    rightPadding: root.clearVisible
                  ? Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing
                  : Kirigami.Units.mediumSpacing

    readonly property bool clearVisible: text.length > 0

    Kirigami.Icon {
        anchors.left: parent.left
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.verticalCenter: parent.verticalCenter
        source: "edit-find"
        implicitWidth: Kirigami.Units.iconSizes.smallMedium
        implicitHeight: Kirigami.Units.iconSizes.smallMedium
        color: Kirigami.Theme.disabledTextColor
    }

    QQC2.ToolButton {
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.rightMargin: Kirigami.Units.smallSpacing
        visible: root.clearVisible
        icon.name: "edit-clear-locationbar-rtl"
        onClicked: root.clear()
    }
}
