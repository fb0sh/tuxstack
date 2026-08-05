pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var volumesModel: null
    property var filesModel: null
    property int detailTabIndex: 0

    readonly property bool hasSelection: root.volumesModel
                                         && root.volumesModel.selectedVolumeName.length > 0

    signal exportRequested(string volumeName)
    signal cloneRequested(string volumeName)
    signal containerRequested(string containerId)
    signal notificationRequested(string message)
    signal filesTabActiveChanged(bool active)

    function activateFilesIfNeeded() {
        if (!root.hasSelection || !root.filesModel)
            return
        if (detailTabs.currentIndex !== 1)
            return
        root.filesModel.setActive(true)
        root.filesModel.openVolume(root.volumesModel.selectedVolumeName)
    }

    onHasSelectionChanged: {
        if (!root.hasSelection) {
            if (root.filesModel)
                root.filesModel.closeVolume()
            return
        }
        root.activateFilesIfNeeded()
    }

    // Completely blank third pane when nothing is selected.
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.hasSelection

        QQC2.TabBar {
            id: detailTabs

            Layout.fillWidth: true
            Layout.preferredHeight: implicitHeight
            Layout.topMargin: Kirigami.Units.smallSpacing
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            currentIndex: root.detailTabIndex

            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Info")
                width: Math.max(implicitWidth, Kirigami.Units.gridUnit * 6)
            }
            QQC2.TabButton {
                text: I18n.i18nd("tuxstack", "Files")
                width: Math.max(implicitWidth, Kirigami.Units.gridUnit * 6)
            }

            onCurrentIndexChanged: {
                root.detailTabIndex = currentIndex
                const filesActive = currentIndex === 1
                root.filesTabActiveChanged(filesActive)
                if (!root.filesModel)
                    return
                root.filesModel.setActive(filesActive)
                if (filesActive && root.volumesModel
                        && root.volumesModel.selectedVolumeName.length > 0) {
                    root.filesModel.openVolume(root.volumesModel.selectedVolumeName)
                }
            }
        }

        Kirigami.Separator {
            Layout.fillWidth: true
            visible: detailTabs.visible
        }

        StackLayout {
            id: detailStack

            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: detailTabs.currentIndex

            VolumeInfoView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                volumesModel: root.volumesModel
                onExportRequested: function(volumeName) {
                    root.exportRequested(volumeName)
                }
                onCloneRequested: function(volumeName) {
                    root.cloneRequested(volumeName)
                }
                onContainerRequested: function(containerId) {
                    root.containerRequested(containerId)
                }
            }

            VolumeFilesView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                filesModel: root.filesModel
                volumesModel: root.volumesModel
                onNotificationRequested: function(message) {
                    root.notificationRequested(message)
                }
            }
        }
    }

    Connections {
        target: root.volumesModel
        function onSelectedVolumeNameChanged() {
            if (!root.volumesModel)
                return
            const name = root.volumesModel.selectedVolumeName
            if (name.length === 0) {
                if (root.filesModel)
                    root.filesModel.closeVolume()
                return
            }
            // Keep Files tab when switching volumes; refresh session if active.
            if (detailTabs.currentIndex === 1 && root.filesModel) {
                root.filesModel.openVolume(name)
            }
        }
    }
}
