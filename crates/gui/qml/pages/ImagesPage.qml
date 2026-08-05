pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Dialogs as Dialogs
import QtQuick.Layouts
import QtCore as QtCore
import org.kde.kirigami as Kirigami
import "../components"
import "../dialogs"

Kirigami.Page {
    id: root

    /**
     * Real ImageListModel / ImagesController facade. This remains nullable so the
     * page and its components can be loaded before the CXX-Qt object is wired.
     */
    property var imagesModel: null
    property var filesModel: null
    property bool controllerInitialized: false
    property string pendingExportImageId: ""
    property string pendingExportName: ""

    signal containerNavigationRequested(string containerId)
    signal notificationRequested(string message)
    signal retryConnectionRequested()
    signal startServiceRequested()
    signal initializationRequested()

    title: qsTr("Images")
    padding: 0

    function safeExportName(displayName, shortId) {
        let base = displayName
        if (!base || base === "<none>:<none>")
            base = "image-" + shortId.replace(/^sha256:/, "")
        base = base.replace(/[^A-Za-z0-9._-]+/g, "-")
                   .replace(/^-+|-+$/g, "")
        if (base.length === 0)
            base = "image"
        return base + ".tar"
    }

    function localPath(url) {
        const value = String(url)
        return value.indexOf("file://") === 0
               ? decodeURIComponent(value.substring(7)) : value
    }

    function notify(message) {
        root.notificationRequested(message)
    }

    function initializeController() {
        if (!root.imagesModel || root.controllerInitialized)
            return
        root.controllerInitialized = true
        root.imagesModel.initialize()
        root.initializationRequested()
    }

    Component.onCompleted: {
        console.info("ImagesPage created")
        root.initializeController()
    }
    onImagesModelChanged: root.initializeController()

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ImageListPanel {
            id: listPanel
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 14
            Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 16,
                                            Math.min(Kirigami.Units.gridUnit * 19,
                                                     root.width * 0.31))
            Layout.maximumWidth: Kirigami.Units.gridUnit * 19
            imagesModel: root.imagesModel

            onPullRequested: pullDialog.open()
            onRetryRequested: root.retryConnectionRequested()
            onRemoveRequested: function(imageId, displayName, shortId, tagsText,
                                        sizeText, usedByCount) {
                removeDialog.prepare(imageId, displayName, shortId, tagsText,
                                     sizeText, usedByCount)
            }
        }

        Kirigami.Separator {
            Layout.fillHeight: true
        }

        ImageDetailPanel {
            id: detailPanel
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 16
            imagesModel: root.imagesModel
            filesModel: root.filesModel

            onExportRequested: function(imageId, displayName, shortId) {
                root.pendingExportImageId = imageId
                root.pendingExportName = root.safeExportName(displayName, shortId)
                const folder = QtCore.StandardPaths.writableLocation(QtCore.StandardPaths.DocumentsLocation)
                saveDialog.currentFolder = folder
                saveDialog.selectedFile = folder + "/" + root.pendingExportName
                saveDialog.open()
            }
            onContainerRequested: function(containerId) {
                root.containerNavigationRequested(containerId)
            }
            onNotificationRequested: function(message) {
                root.notify(message)
            }
        }
    }

    PullImageDialog {
        id: pullDialog
        imagesModel: root.imagesModel
    }

    RemoveImageDialog {
        id: removeDialog
        removing: root.imagesModel
                  && root.imagesModel.removingImageId === removeDialog.imageId
        errorMessage: root.imagesModel ? root.imagesModel.removeErrorMessage : ""
        onRemovalRequested: function(imageId, force, pruneChildren) {
            if (root.imagesModel)
                root.imagesModel.removeImage(imageId, force, pruneChildren)
        }
    }

    Dialogs.FileDialog {
        id: saveDialog
        title: qsTr("Export Docker Image")
        fileMode: Dialogs.FileDialog.SaveFile
        nameFilters: [qsTr("Tar archives (*.tar)"), qsTr("All files (*)")]
        defaultSuffix: "tar"
        acceptLabel: qsTr("Export")
        onAccepted: {
            const path = root.localPath(selectedFile)
            exportDialog.showFor(path)
            if (root.imagesModel)
                root.imagesModel.exportImage(root.pendingExportImageId, path)
        }
    }

    ExportImageDialog {
        id: exportDialog
        imagesModel: root.imagesModel
    }

    Connections {
        target: root.imagesModel
        ignoreUnknownSignals: true

        function onImageRemoved(displayName) {
            removeDialog.close()
            root.notify(qsTr("Image “%1” removed.").arg(displayName))
        }

        function onPullCompleted(imageReference) {
            root.notify(qsTr("Image “%1” pulled.").arg(imageReference))
        }

        function onExportCompleted(destinationPath) {
            root.notify(qsTr("Image exported to %1").arg(destinationPath))
        }
    }

    Connections {
        target: root.filesModel
        ignoreUnknownSignals: true

        function onStartServiceRequested() { root.startServiceRequested() }
    }
}
