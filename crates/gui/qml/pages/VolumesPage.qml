pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components"
import "../dialogs"

Kirigami.Page {
    id: root

    property var volumesModel: null
    property var initializedModel: null

    signal initializationRequested()
    signal retryConnectionRequested()
    signal containerNavigationRequested(string containerId)
    signal notificationRequested(string message)

    title: I18n.i18nd("tuxstack", "Volumes")
    padding: 0

    function initializeModel() {
        if (!root.volumesModel || root.initializedModel === root.volumesModel)
            return
        root.initializedModel = root.volumesModel
        root.volumesModel.initialize()
        root.initializationRequested()
    }

    function notify(message) {
        root.notificationRequested(message)
    }

    Component.onCompleted: root.initializeModel()
    onVolumesModelChanged: root.initializeModel()

    RowLayout {
        anchors.fill: parent
        spacing: 0

        VolumeListPanel {
            id: listPanel

            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 14
            Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 16,
                                            Math.min(Kirigami.Units.gridUnit * 19,
                                                     root.width * 0.31))
            Layout.maximumWidth: Kirigami.Units.gridUnit * 19
            volumesModel: root.volumesModel

            onCreateRequested: createDialog.prepare()
            onRetryRequested: {
                if (root.volumesModel)
                    root.volumesModel.refresh()
                root.retryConnectionRequested()
            }
            onRemoveRequested: function(volumeName) {
                if (root.volumesModel)
                    root.volumesModel.prepareRemoveVolume(volumeName)
            }
            onPruneRequested: {
                if (root.volumesModel)
                    root.volumesModel.preparePruneVolumes()
            }
        }

        Kirigami.Separator {
            Layout.fillHeight: true
        }

        VolumeDetailPanel {
            id: detailPanel

            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 16
            volumesModel: root.volumesModel

            onExportRequested: function(volumeName) {
                exportDialog.prepare(volumeName)
            }
            onCloneRequested: function(volumeName) {
                cloneDialog.prepare(volumeName)
            }
            onContainerRequested: function(containerId) {
                if (root.volumesModel)
                    root.volumesModel.navigateToContainer(containerId)
                root.containerNavigationRequested(containerId)
            }
        }
    }

    CreateVolumeDialog {
        id: createDialog
        volumesModel: root.volumesModel
    }

    RemoveVolumeDialog {
        id: removeDialog
        volumesModel: root.volumesModel
    }

    PruneVolumesDialog {
        id: pruneDialog
        volumesModel: root.volumesModel
    }

    ExportVolumeDialog {
        id: exportDialog
        volumesModel: root.volumesModel
    }

    CloneVolumeDialog {
        id: cloneDialog
        volumesModel: root.volumesModel
    }

    Connections {
        target: root.volumesModel
        ignoreUnknownSignals: true

        function onRemovePrepared(volumeName, driver, sizeText, usedByCount, mountpoint) {
            removeDialog.prepare(volumeName, driver, sizeText, usedByCount, mountpoint)
        }

        function onRemovePreparationFailed(message) {
            root.notify(message)
        }

        function onPrunePrepared() {
            pruneDialog.prepare()
        }

        function onPrunePreparationFailed(message) {
            root.notify(message)
        }

        function onVolumeCreated(volumeName) {
            createDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Volume “%1” created.").arg(volumeName))
        }

        function onVolumeRemoved(volumeName) {
            removeDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Volume “%1” removed.").arg(volumeName))
        }

        function onVolumesPruned(removedCount, reclaimedSizeText, unknownSizeCount) {
            pruneDialog.close()
            if (Number(unknownSizeCount) > 0) {
                root.notify(I18n.i18nd("tuxstack", "%1 unused volumes removed; %2 known data reclaimed. Some removed volumes had unknown size.")
                            .arg(removedCount).arg(reclaimedSizeText))
            } else {
                root.notify(I18n.i18nd("tuxstack", "%1 unused volumes removed; %2 reclaimed.")
                            .arg(removedCount).arg(reclaimedSizeText))
            }
        }

        function onExportCompleted(volumeName, destinationPath) {
            exportDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Volume “%1” exported to %2.")
                        .arg(volumeName).arg(destinationPath))
        }

        function onCloneCompleted(sourceVolume, targetVolume) {
            cloneDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Volume “%1” cloned as “%2”.")
                        .arg(sourceVolume).arg(targetVolume))
        }
    }
}
