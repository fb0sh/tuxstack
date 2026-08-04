pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app
import "../components"
import "../dialogs"

Kirigami.Page {
    id: root

    property var networksModel: null
    property var initializedModel: null

    signal initializationRequested()
    signal retryConnectionRequested()
    signal notificationRequested(string message)

    title: I18n.i18nd("tuxstack", "Networks")
    padding: 0

    function initializeModel() {
        if (!root.networksModel || root.initializedModel === root.networksModel)
            return

        root.initializedModel = root.networksModel
        root.networksModel.initialize()
        root.initializationRequested()
    }

    function notify(message) {
        root.notificationRequested(message)
    }

    Component.onCompleted: root.initializeModel()
    onNetworksModelChanged: root.initializeModel()

    RowLayout {
        anchors.fill: parent
        spacing: 0

        NetworkListPanel {
            id: listPanel

            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 14
            Layout.preferredWidth: Math.max(Kirigami.Units.gridUnit * 16,
                                            Math.min(Kirigami.Units.gridUnit * 19,
                                                     root.width * 0.31))
            Layout.maximumWidth: Kirigami.Units.gridUnit * 19
            networksModel: root.networksModel

            onCreateRequested: createDialog.prepare()
            onRetryRequested: root.retryConnectionRequested()
            onRemoveRequested: function(networkId) {
                if (root.networksModel)
                    root.networksModel.prepareRemoveNetwork(networkId)
            }
        }

        Kirigami.Separator {
            Layout.fillHeight: true
        }

        NetworkDetailPanel {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumWidth: Kirigami.Units.gridUnit * 16
            networksModel: root.networksModel
        }
    }

    CreateNetworkDialog {
        id: createDialog
        networksModel: root.networksModel
    }

    RemoveNetworkDialog {
        id: removeDialog
        removing: root.networksModel
                  && root.networksModel.removingNetworkId === removeDialog.networkId
        errorMessage: root.networksModel ? root.networksModel.removeErrorMessage : ""

        onRemovalRequested: function(networkId) {
            if (root.networksModel)
                root.networksModel.removeNetwork(networkId)
        }
    }

    Connections {
        target: root.networksModel
        ignoreUnknownSignals: true

        function onRemovePrepared(networkId, name, shortId, containerCount) {
            removeDialog.prepare(networkId, name, shortId, containerCount)
        }

        function onRemovePreparationFailed(message) {
            root.notify(message)
        }

        function onNetworkCreated(name) {
            createDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Network “%1” created.").arg(name))
        }

        function onNetworkRemoved(name) {
            removeDialog.close()
            root.notify(I18n.i18nd("tuxstack", "Network “%1” removed.").arg(name))
        }
    }
}
