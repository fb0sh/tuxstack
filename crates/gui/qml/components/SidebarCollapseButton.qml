import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

/** Compact tool button used to collapse or expand the application sidebar. */
QQC2.ToolButton {
    id: root

    property bool collapsed: false
    signal triggered()

    icon.name: collapsed ? "sidebar-expand" : "sidebar-collapse"
    display: QQC2.AbstractButton.IconOnly
    Accessible.name: collapsed ? qsTr("Expand sidebar") : qsTr("Collapse sidebar")
    QQC2.ToolTip.text: Accessible.name
    QQC2.ToolTip.visible: hovered
    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay

    onClicked: triggered()
}
