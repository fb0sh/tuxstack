import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/**
 * Networks page: real network list.
 * Create/connect/disconnect operations are planned.
 */
Kirigami.Page {
    id: root

    property var networksModel: null

    title: i18nd("tuxstack", "Networks")

    function refresh() {
        if (networksModel) networksModel.refresh()
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
                    if (networksModel) networksModel.searchText = text
                    root.refresh()
                }
            }
        }

        ErrorBanner {
            Layout.fillWidth: true
            textMessage: (networksModel && networksModel.status === 4) ? networksModel.statusText : ""
        }

        LoadingView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: networksModel && networksModel.status === 1
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: networksModel && networksModel.status === 3
            iconName: "network-server"
            title: i18nd("tuxstack", "No networks")
            message: i18nd("tuxstack", "Docker always provides a default bridge network.")
        }

        EmptyState {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: networksModel && networksModel.status === 5
            iconName: "network-offline"
            title: i18nd("tuxstack", "Docker unavailable")
            message: networksModel ? networksModel.statusText : ""
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: networksModel && (networksModel.status === 2 || networksModel.status === 0)
            model: networksModel
            ScrollBar.vertical: QQC2.ScrollBar {}
            spacing: 1

            delegate: Kirigami.AbstractCard {
                width: ListView.view.width
                contentItem: RowLayout {
                    spacing: Kirigami.Units.mediumSpacing
                    Layout.margins: Kirigami.Units.mediumSpacing

                    Kirigami.Icon {
                        source: "network-server"
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
                                text: model.internal ? i18nd("tuxstack", "internal") : ""
                                color: Kirigami.Theme.neutralTextColor
                                font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                            }
                            QQC2.Label {
                                text: model.ipv6 ? "IPv6" : ""
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
