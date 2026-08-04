import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    default property alias contentData: body.data
    property string title: ""

    spacing: Kirigami.Units.largeSpacing

    Kirigami.Heading {
        Layout.fillWidth: true
        text: root.title
        level: 2
    }

    ColumnLayout {
        id: body
        Layout.fillWidth: true
        spacing: 0
    }
}
