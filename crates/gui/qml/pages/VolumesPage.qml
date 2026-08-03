import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Volumes page: real volume list.
 */
Kirigami.Page {
    id: root

    property var volumesModel: null

    title: i18nd("tuxstack", "Volumes")

    function refresh() {
        if (volumesModel) volumesModel.refresh()
    }

    Component.onCompleted: refresh()
    onIsCurrentPageChanged: {
        if (isCurrentPage) refresh()
    }

    actions: [
        Kirigami.Action {
            icon.name: "view-refresh"
            text: i18nd("tuxstack", "Refresh")
            onTriggered: root.refresh()
        }
    ]

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.mediumSpacing
            spacing: Kirigami.Units.mediumSpacing

            SearchField {
                Layout.fillWidth: true
                onTextChanged: {
                    if (volumesModel) volumesModel.searchText = text
                    root.refresh()
                }
            }
        }

        ErrorBanner {
            Layout.fillWidth: true
            textMessage: (volumesModel && volumesModel.status === 4) ? volumesModel.statusText : ""
        }

        LoadingView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: volumesModel && volumesModel.status === 1
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: volumesModel && volumesModel.status === 3
            iconName: "drive-harddisk"
            title: i18nd("tuxstack", "No volumes")
            message: i18nd("tuxstack", "Volumes created by containers will appear here.")
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: volumesModel && volumesModel.status === 5
            iconName: "network-offline"
            title: i18nd("tuxstack", "Docker unavailable")
            message: volumesModel ? volumesModel.statusText : ""
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: volumesModel && (volumesModel.status === 2 || volumesModel.status === 0)
            model: volumesModel
            ScrollBar.vertical: QQC2.ScrollBar {}
            spacing: 1

            delegate: Kirigami.AbstractCard {
                width: ListView.view.width
                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing
                    Layout.margins: Kirigami.Units.mediumSpacing

                    Kirigami.Icon {
                        source: "drive-harddisk"
                        implicitWidth: Kirigami.Units.iconSizes.medium
                        implicitHeight: Kirigami.Units.iconSizes.medium
                        color: Kirigami.Theme.textColor
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        QQC2.Label {
                            text: model.name
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        RowLayout {
                            spacing: Kirigami.Units.largeSpacing
                            QQC2.Label {
                                text: model.driver
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: model.scope
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: i18nd("tuxstack", "Created: %1", model.createdAt)
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                        }
                        QQC2.Label {
                            text: model.mountpoint
                            color: Kirigami.Theme.disabledTextColor
                            font.family: "monospace"
                            font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            elide: Text.ElideRight
                        }
                    }
                }
            }
        }
    }
}
