import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.tuxstack.app

QQC2.Menu {
    id: root

    property string containerId: ""
    property string containerName: ""
    property string image: ""
    property string state: "unknown"
    property var publishedPorts: []
    property var mounts: []
    property bool logsCapability: false
    property bool terminalCapability: false
    property bool filesCapability: false
    property bool mountNavigationCapability: false
    property bool localEndpoint: false

    signal startRequested(string id)
    signal stopRequested(string id)
    signal restartRequested(string id)
    signal killRequested(string id)
    signal pauseRequested(string id)
    signal unpauseRequested(string id)
    signal removeRequested(string id)
    signal renameRequested(string id)
    signal logsRequested(string id)
    signal terminalRequested(string id)
    signal inAppTerminalRequested(string id)
    signal filesRequested(string id)
    signal browserRequested(string url)
    signal mountRequested(string type, string source, string destination, string volumeName)
    signal copyRequested(string value)

    readonly property bool running: root.state === "running"
    readonly property bool restarting: root.state === "restarting"
    readonly property bool paused: root.state === "paused"
    readonly property bool stopped: !root.running && !root.paused

    QQC2.MenuItem {
        visible: root.stopped
        text: I18n.i18nd("tuxstack", "Start")
        icon.name: "media-playback-start"
        onTriggered: root.startRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.running
        text: I18n.i18nd("tuxstack", "Stop")
        icon.name: "media-playback-stop"
        onTriggered: root.stopRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.running
        text: I18n.i18nd("tuxstack", "Restart")
        icon.name: "view-refresh"
        onTriggered: root.restartRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.running || root.paused
        text: I18n.i18nd("tuxstack", "Kill…")
        icon.name: "process-stop"
        onTriggered: root.killRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.running
        text: I18n.i18nd("tuxstack", "Pause")
        icon.name: "media-playback-pause"
        onTriggered: root.pauseRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.paused
        text: I18n.i18nd("tuxstack", "Resume")
        icon.name: "media-playback-start"
        onTriggered: root.unpauseRequested(root.containerId)
    }
    QQC2.MenuItem {
        text: I18n.i18nd("tuxstack", "Rename…")
        icon.name: "edit-rename"
        onTriggered: root.renameRequested(root.containerId)
    }
    QQC2.MenuItem {
        text: I18n.i18nd("tuxstack", "Delete…")
        icon.name: "edit-delete"
        onTriggered: root.removeRequested(root.containerId)
    }

    QQC2.MenuSeparator {
        visible: root.logsCapability || root.terminalCapability || root.filesCapability
    }
    QQC2.MenuItem {
        visible: root.logsCapability
        text: I18n.i18nd("tuxstack", "Logs")
        icon.name: "view-list-text"
        onTriggered: root.logsRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.terminalCapability
        enabled: root.running
        text: I18n.i18nd("tuxstack", "Open Terminal")
        icon.name: "utilities-terminal"
        onTriggered: root.terminalRequested(root.containerId)
        QQC2.ToolTip.text: root.running ? "" : root.paused
                                      ? I18n.i18nd("tuxstack", "Resume the container before opening a terminal.")
                                      : root.restarting
                                      ? I18n.i18nd("tuxstack", "The container is currently restarting.")
                                      : I18n.i18nd("tuxstack", "Start the container before opening a terminal.")
    }
    QQC2.MenuItem {
        visible: root.terminalCapability
        enabled: root.running
        text: I18n.i18nd("tuxstack", "Terminal")
        icon.name: "utilities-terminal"
        onTriggered: root.inAppTerminalRequested(root.containerId)
    }
    QQC2.MenuItem {
        visible: root.filesCapability
        text: I18n.i18nd("tuxstack", "Files")
        icon.name: "folder"
        onTriggered: root.filesRequested(root.containerId)
    }

    QQC2.Menu {
        id: browserMenu
        visible: browserInstantiator.count > 0
        enabled: visible
        title: I18n.i18nd("tuxstack", "Open in Browser")
        icon.name: "internet-web-browser"

        Instantiator {
            id: browserInstantiator
            model: root.publishedPorts || []
            delegate: QQC2.MenuItem {
                required property var modelData
                visible: String(modelData.browserUrl || "").length > 0
                text: String(modelData.browserUrl || "")
                onTriggered: root.browserRequested(String(modelData.browserUrl || ""))
            }
            onObjectAdded: function(index, object) { browserMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { browserMenu.removeItem(object) }
        }
    }

    QQC2.Menu {
        id: mountMenu
        visible: root.mountNavigationCapability && mountInstantiator.count > 0
        enabled: visible
        title: I18n.i18nd("tuxstack", "Mounts")
        icon.name: "drive-harddisk"

        Instantiator {
            id: mountInstantiator
            model: root.mounts || []
            delegate: QQC2.MenuItem {
                required property var modelData
                text: String(modelData.destination || "") + " → " + String(modelData.source || modelData.type || "")
                enabled: String(modelData.type || "") === "volume"
                         || (root.localEndpoint && String(modelData.type || "") === "bind")
                onTriggered: root.mountRequested(String(modelData.type || ""), String(modelData.source || ""), String(modelData.destination || ""), String(modelData.volumeName || ""))
            }
            onObjectAdded: function(index, object) { mountMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { mountMenu.removeItem(object) }
        }
    }

    QQC2.MenuSeparator { }
    QQC2.MenuItem {
        text: I18n.i18nd("tuxstack", "Copy ID")
        icon.name: "edit-copy"
        onTriggered: root.copyRequested(root.containerId)
    }
    QQC2.MenuItem {
        text: I18n.i18nd("tuxstack", "Copy Name")
        icon.name: "edit-copy"
        onTriggered: root.copyRequested(root.containerName)
    }
    QQC2.MenuItem {
        text: I18n.i18nd("tuxstack", "Copy Image")
        icon.name: "edit-copy"
        onTriggered: root.copyRequested(root.image)
    }
}
