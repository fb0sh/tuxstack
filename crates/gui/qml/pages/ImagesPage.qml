import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Images page: real image list with search.
 * Pull/build/tag/push/prune are planned.
 */
Kirigami.Page {
    id: root

    property var imagesModel: null

    title: i18nd("tuxstack", "Images")

    function refresh() {
        if (imagesModel) imagesModel.refresh()
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
        },
        Kirigami.Action {
            icon.name: "download"
            text: i18nd("tuxstack", "Pull (planned)")
            enabled: false
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
                    if (imagesModel) imagesModel.searchText = text
                    root.refresh()
                }
            }
        }

        ErrorBanner {
            Layout.fillWidth: true
            textMessage: (imagesModel && imagesModel.status === 4) ? imagesModel.statusText : ""
        }

        LoadingView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: imagesModel && imagesModel.status === 1
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: imagesModel && imagesModel.status === 3
            iconName: "image-x-generic"
            title: i18nd("tuxstack", "No images")
            message: i18nd("tuxstack", "Pull images with `docker pull` from the terminal.")
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: imagesModel && imagesModel.status === 5
            iconName: "network-offline"
            title: i18nd("tuxstack", "Docker unavailable")
            message: imagesModel ? imagesModel.statusText : ""
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: imagesModel && (imagesModel.status === 2 || imagesModel.status === 0)
            model: imagesModel
            ScrollBar.vertical: QQC2.ScrollBar {}
            spacing: 1

            delegate: Kirigami.AbstractCard {
                width: ListView.view.width
                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing
                    Layout.margins: Kirigami.Units.mediumSpacing

                    Kirigami.Icon {
                        source: "image-x-generic"
                        implicitWidth: Kirigami.Units.iconSizes.medium
                        implicitHeight: Kirigami.Units.iconSizes.medium
                        color: Kirigami.Theme.textColor
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        QQC2.Label {
                            text: model.tags
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        RowLayout {
                            spacing: Kirigami.Units.largeSpacing
                            QQC2.Label {
                                text: model.shortId
                                color: Kirigami.Theme.disabledTextColor
                                font.family: "monospace"
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: i18nd("tuxstack", "Size: %1", model.size)
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: i18nd("tuxstack", "Created: %1", model.createdAt)
                                color: Kirigami.Theme.disabledTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                        }
                    }
                }
            }
        }
    }
}
