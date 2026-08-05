import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

/*
 * Dedicated terminal surface. Docker TTY bytes and ANSI/VT state stay in the
 * Rust ContainerTerminalModel; this component renders only interpreted screen
 * rows and cursor coordinates, never a plain editable transcript.
 */
FocusScope {
    id: root

    property var terminalModel: null
    property font terminalFont: Qt.font({
        family: "monospace",
        pixelSize: Math.max(12, Kirigami.Theme.defaultFont.pixelSize)
    })
    readonly property bool ready: terminalModel
                                  && terminalModel.terminalState === "ready"
    readonly property real cellWidth: Math.max(1, cellMetrics.advanceWidth)
    readonly property real cellHeight: Math.ceil(fontMetrics.height)
    signal viewLogsRequested()
    signal viewFilesRequested()

    function paste(text) {
        if (terminalModel && text.length > 0)
            terminalModel.paste(text)
        terminalSurface.forceActiveFocus(Qt.ShortcutFocusReason)
    }

    function pasteFromClipboard() {
        clipboardInput.visible = true
        clipboardInput.text = ""
        clipboardInput.forceActiveFocus(Qt.ShortcutFocusReason)
        clipboardInput.paste()
        const value = clipboardInput.text
        clipboardInput.text = ""
        clipboardInput.visible = false
        root.paste(value)
    }

    function sendSpecialKey(key) {
        if (terminalModel)
            terminalModel.sendKey(key)
    }

    function scheduleResize() {
        if (ready && terminalViewport.width > 0 && terminalViewport.height > 0)
            resizeDebounce.restart()
    }

    onTerminalModelChanged: {
        if (terminalModel)
            terminalModel.setActive(visible)
        scheduleResize()
    }
    onReadyChanged: scheduleResize()
    onVisibleChanged: {
        if (terminalModel)
            terminalModel.setActive(visible)
        if (visible)
            Qt.callLater(terminalSurface.forceActiveFocus)
    }
    Component.onCompleted: {
        if (terminalModel)
            terminalModel.setActive(visible)
        scheduleResize()
    }

    TextMetrics {
        id: cellMetrics
        font: root.terminalFont
        text: "M"
    }

    FontMetrics {
        id: fontMetrics
        font: root.terminalFont
    }

    Timer {
        id: resizeDebounce
        interval: 90
        repeat: false
        onTriggered: {
            if (!root.ready || !root.terminalModel)
                return
            const columns = Math.max(2, Math.floor(terminalViewport.width / root.cellWidth))
            const rows = Math.max(2, Math.floor(terminalViewport.height / root.cellHeight))
            root.terminalModel.resize(rows, columns)
        }
    }

    Timer {
        id: cursorBlink
        interval: 550
        repeat: true
        running: root.ready && root.activeFocus
        onTriggered: cursor.blinkOn = !cursor.blinkOn
        onRunningChanged: if (!running) cursor.blinkOn = true
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.terminalModel
                     && root.terminalModel.terminalState === "error"
            type: Kirigami.MessageType.Error
            text: root.terminalModel ? root.terminalModel.errorMessage : ""

            actions: [
                Kirigami.Action {
                    text: I18n.i18nd("tuxstack", "Retry")
                    icon.name: "view-refresh"
                    onTriggered: if (root.terminalModel) root.terminalModel.retry()
                }
            ]
        }

        Rectangle {
            id: terminalFrame
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: "#101216"
            border.width: terminalSurface.activeFocus ? 2 : 1
            border.color: terminalSurface.activeFocus
                          ? Kirigami.Theme.focusColor
                          : Qt.rgba(1, 1, 1, 0.22)
            radius: Kirigami.Units.cornerRadius
            clip: true

            FocusScope {
                id: terminalSurface
                anchors.fill: parent
                anchors.margins: terminalFrame.border.width
                focus: true
                activeFocusOnTab: true
                Accessible.name: I18n.i18nd("tuxstack", "Container terminal")
                Accessible.description: root.ready
                                        ? I18n.i18nd("tuxstack", "Interactive terminal connected to the selected container")
                                        : I18n.i18nd("tuxstack", "Container terminal is not connected")
                Keys.priority: Keys.BeforeItem

                Keys.onPressed: function(event) {
                    if (!root.ready || !root.terminalModel)
                        return

                    const control = (event.modifiers & Qt.ControlModifier) !== 0
                    const shift = (event.modifiers & Qt.ShiftModifier) !== 0
                    const alt = (event.modifiers & Qt.AltModifier) !== 0
                    const meta = (event.modifiers & Qt.MetaModifier) !== 0

                    if (control && event.key === Qt.Key_V) {
                        root.pasteFromClipboard()
                        event.accepted = true
                        return
                    }
                    if (control && event.key >= Qt.Key_A && event.key <= Qt.Key_Z) {
                        root.sendSpecialKey("Ctrl+" + String.fromCharCode(event.key))
                        event.accepted = true
                        return
                    }

                    let special = ""
                    switch (event.key) {
                    case Qt.Key_Return:
                    case Qt.Key_Enter: special = "Enter"; break
                    case Qt.Key_Backspace: special = "Backspace"; break
                    case Qt.Key_Tab:
                    case Qt.Key_Backtab: special = "Tab"; break
                    case Qt.Key_Up: special = "Up"; break
                    case Qt.Key_Down: special = "Down"; break
                    case Qt.Key_Left: special = "Left"; break
                    case Qt.Key_Right: special = "Right"; break
                    case Qt.Key_Home: special = "Home"; break
                    case Qt.Key_End: special = "End"; break
                    case Qt.Key_PageUp: special = "PageUp"; break
                    case Qt.Key_PageDown: special = "PageDown"; break
                    case Qt.Key_Delete: special = "Delete"; break
                    case Qt.Key_Insert: special = "Insert"; break
                    case Qt.Key_Escape: special = "Escape"; break
                    }
                    if (special.length > 0) {
                        root.sendSpecialKey(special)
                        event.accepted = true
                    } else if (!control && !alt && !meta && event.text.length > 0) {
                        root.terminalModel.sendText(event.text)
                        event.accepted = true
                    }
                }

                Item {
                    id: terminalViewport
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.smallSpacing
                    clip: true
                    onWidthChanged: root.scheduleResize()
                    onHeightChanged: root.scheduleResize()

                    ListView {
                        id: screenRows
                        anchors.fill: parent
                        model: root.terminalModel
                        interactive: false
                        boundsBehavior: Flickable.StopAtBounds
                        spacing: 0

                        delegate: Item {
                            id: screenRow
                            required property string text
                            width: screenRows.width
                            height: root.cellHeight

                            Text {
                                anchors.fill: parent
                                text: screenRow.text
                                textFormat: Text.PlainText
                                font: root.terminalFont
                                color: "#e8e8e8"
                                elide: Text.ElideNone
                                wrapMode: Text.NoWrap
                                renderType: Text.NativeRendering
                            }
                        }
                    }

                    Rectangle {
                        id: cursor
                        property bool blinkOn: true
                        x: root.terminalModel
                           ? root.terminalModel.cursorColumn * root.cellWidth : 0
                        y: root.terminalModel
                           ? root.terminalModel.cursorRow * root.cellHeight : 0
                        width: Math.max(1, root.cellWidth)
                        height: root.cellHeight
                        color: Qt.rgba(0.92, 0.92, 0.92, 0.72)
                        visible: root.ready && root.terminalModel.cursorVisible
                                 && blinkOn && root.activeFocus
                                 && x < terminalViewport.width
                                 && y < terminalViewport.height
                    }

                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.LeftButton
                        cursorShape: Qt.IBeamCursor
                        onClicked: terminalSurface.forceActiveFocus(Qt.MouseFocusReason)
                        onWheel: function(wheel) {
                            if (!root.terminalModel)
                                return
                            const steps = Math.max(1, Math.round(Math.abs(wheel.angleDelta.y) / 120))
                            root.terminalModel.scrollLines(wheel.angleDelta.y > 0 ? steps * 3 : -steps * 3)
                            wheel.accepted = true
                        }
                    }
                }

                QQC2.BusyIndicator {
                    anchors.centerIn: parent
                    visible: root.terminalModel
                             && root.terminalModel.terminalState === "connecting"
                    running: visible
                }

                ColumnLayout {
                    anchors.centerIn: parent
                    width: Math.min(parent.width - Kirigami.Units.largeSpacing * 2,
                                    Kirigami.Units.gridUnit * 28)
                    visible: !root.ready && root.terminalModel
                             && root.terminalModel.terminalState !== "connecting"
                             && root.terminalModel.terminalState !== "error"
                    spacing: Kirigami.Units.smallSpacing

                    Kirigami.Icon {
                        Layout.alignment: Qt.AlignHCenter
                        source: "utilities-terminal"
                        implicitWidth: Kirigami.Units.iconSizes.large
                        implicitHeight: implicitWidth
                        color: "#e8e8e8"
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                        color: "#e8e8e8"
                        text: !root.terminalModel || root.terminalModel.containerId.length === 0
                              ? I18n.i18nd("tuxstack", "Select a container to open a terminal.")
                              : (root.terminalModel.errorMessage.length > 0
                                 ? root.terminalModel.errorMessage
                                 : (root.terminalModel.terminalState === "exited"
                                    ? I18n.i18nd("tuxstack", "The terminal session has exited.")
                                    : I18n.i18nd("tuxstack", "Start or resume the container to open a terminal.")))
                    }
                    QQC2.Button {
                        Layout.alignment: Qt.AlignHCenter
                        visible: root.terminalModel
                                 && root.terminalModel.running
                                 && root.terminalModel.terminalState === "exited"
                        text: I18n.i18nd("tuxstack", "Retry")
                        icon.name: "view-refresh"
                        onClicked: root.terminalModel.retry()
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: root.terminalModel
                     && root.terminalModel.errorKind === "shell_not_found"
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "Copy Container ID")
                icon.name: "edit-copy"
                onClicked: {
                    clipboardInput.visible = true
                    clipboardInput.text = root.terminalModel.containerId
                    clipboardInput.selectAll()
                    clipboardInput.copy()
                    clipboardInput.deselect()
                    clipboardInput.text = ""
                    clipboardInput.visible = false
                    terminalSurface.forceActiveFocus(Qt.ShortcutFocusReason)
                }
            }
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "View Logs")
                icon.name: "view-list-text"
                onClicked: root.viewLogsRequested()
            }
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "View Files")
                icon.name: "folder"
                onClicked: root.viewFilesRequested()
            }
            Item { Layout.fillWidth: true }
        }

        QQC2.Label {
            Layout.fillWidth: true
            visible: root.ready
            text: root.terminalModel && root.terminalModel.shell.length > 0
                  ? I18n.i18nd("tuxstack", "Shell: %1", root.terminalModel.shell)
                  : ""
            color: Kirigami.Theme.disabledTextColor
            font.pointSize: Kirigami.Theme.smallFont.pointSize
            elide: Text.ElideMiddle
        }
    }

    // Clipboard gateway only. It has no geometry and is never the terminal
    // renderer; all terminal display and ANSI interpretation happen in Rust.
    TextInput {
        id: clipboardInput
        width: 0
        height: 0
        opacity: 0
        visible: false
    }
}
