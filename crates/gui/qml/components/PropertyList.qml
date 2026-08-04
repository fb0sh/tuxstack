import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Flat, dense property list used by image detail sections.
 * Rows provide their own responsive key/value layout; this component adds
 * only KDE spacing and deliberately has no card, border, or background.
 */
ColumnLayout {
    id: root

    default property alias contentData: body.data

    spacing: 0

    ColumnLayout {
        id: body
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
    }
}
