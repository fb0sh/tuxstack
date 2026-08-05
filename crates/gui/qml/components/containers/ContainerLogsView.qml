import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Item {
    id: root

    property var logsModel: null
    property bool autoScroll: true

    function copyViewport() {
        clipboardBuffer.text = root.logsModel ? root.logsModel.viewportText() : ""
        clipboardBuffer.selectAll()
        clipboardBuffer.copy()
        clipboardBuffer.deselect()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.TextField {
                Layout.fillWidth: true
                placeholderText: I18n.i18nd("tuxstack", "Search logs…")
                text: root.logsModel ? root.logsModel.searchQuery : ""
                selectByMouse: true
                onTextEdited: if (root.logsModel) root.logsModel.setSearch(text)
            }
            QQC2.ToolButton {
                checkable: true
                checked: root.logsModel ? root.logsModel.follow : false
                text: I18n.i18nd("tuxstack", "Follow")
                icon.name: "go-bottom"
                display: QQC2.AbstractButton.TextBesideIcon
                onToggled: if (root.logsModel) root.logsModel.updateFollow(checked)
            }
            QQC2.ToolButton {
                checkable: true
                checked: root.logsModel ? root.logsModel.timestamps : true
                text: I18n.i18nd("tuxstack", "Timestamps")
                icon.name: "view-calendar-time-spent"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                onToggled: if (root.logsModel) root.logsModel.updateTimestamps(checked)
            }
            QQC2.ToolButton {
                checkable: true
                checked: root.logsModel ? root.logsModel.paused : false
                text: checked && root.logsModel && root.logsModel.pendingCount > 0
                      ? I18n.i18nd("tuxstack", "Resume (%1 queued)", root.logsModel.pendingCount)
                      : (checked ? I18n.i18nd("tuxstack", "Resume") : I18n.i18nd("tuxstack", "Pause display"))
                icon.name: checked ? "media-playback-start" : "media-playback-pause"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                onToggled: if (root.logsModel) root.logsModel.updatePaused(checked)
            }
            QQC2.ToolButton {
                checkable: true
                checked: root.logsModel ? root.logsModel.wrap : false
                text: I18n.i18nd("tuxstack", "Wrap lines")
                icon.name: "format-text-direction-horizontal"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                onToggled: if (root.logsModel) root.logsModel.updateWrap(checked)
            }
            QQC2.ToolButton {
                text: I18n.i18nd("tuxstack", "Copy current viewport")
                icon.name: "edit-copy"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                enabled: root.logsModel && root.logsModel.count > 0
                onClicked: root.copyViewport()
            }
            QQC2.ToolButton {
                text: I18n.i18nd("tuxstack", "Save current viewport…")
                icon.name: "document-save"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                enabled: root.logsModel && root.logsModel.count > 0
                onClicked: saveDialog.open()
            }
            QQC2.ToolButton {
                text: I18n.i18nd("tuxstack", "Clear View")
                icon.name: "edit-clear-all"
                display: QQC2.AbstractButton.IconOnly
                QQC2.ToolTip.text: text
                QQC2.ToolTip.visible: hovered
                enabled: root.logsModel && root.logsModel.count > 0
                onClicked: if (root.logsModel) root.logsModel.clearViewport()
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.CheckBox {
                id: stdoutToggle
                property bool syncing: false
                text: I18n.i18nd("tuxstack", "stdout")
                checked: root.logsModel ? root.logsModel.stdout : true
                onToggled: if (root.logsModel && !syncing) {
                    root.logsModel.updateStdout(checked)
                    if (checked !== root.logsModel.stdout) {
                        syncing = true
                        checked = Qt.binding(function() {
                            return root.logsModel ? root.logsModel.stdout : true
                        })
                        syncing = false
                    }
                }
            }
            QQC2.CheckBox {
                id: stderrToggle
                property bool syncing: false
                text: I18n.i18nd("tuxstack", "stderr")
                checked: root.logsModel ? root.logsModel.stderr : true
                onToggled: if (root.logsModel && !syncing) {
                    root.logsModel.updateStderr(checked)
                    if (checked !== root.logsModel.stderr) {
                        syncing = true
                        checked = Qt.binding(function() {
                            return root.logsModel ? root.logsModel.stderr : true
                        })
                        syncing = false
                    }
                }
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Tail:")
            }
            QQC2.ComboBox {
                id: tailSelector
                model: ["100", "500", "1000", "5000", "all"]
                currentIndex: root.logsModel ? Math.max(0, model.indexOf(String(root.logsModel.tail))) : 2
                onActivated: if (root.logsModel) root.logsModel.updateTail(currentText)
                QQC2.ToolTip.text: I18n.i18nd("tuxstack", "Initial lines from the end of Docker history")
                QQC2.ToolTip.visible: hovered
            }
            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Since:")
            }
            QQC2.ComboBox {
                id: sinceSelector
                Layout.preferredWidth: Kirigami.Units.gridUnit * 7
                editable: true
                model: [I18n.i18nd("tuxstack", "All time"), "5m", "1h", "24h"]
                editText: root.logsModel && root.logsModel.since.length > 0
                          ? root.logsModel.since : model[0]
                onActivated: function(index) {
                    if (root.logsModel)
                        root.logsModel.updateSince(index === 0 ? "" : currentText)
                }
                onAccepted: if (root.logsModel)
                    root.logsModel.updateSince(editText === model[0] ? "" : editText)
                QQC2.ToolTip.text: I18n.i18nd("tuxstack", "All time, Unix timestamp, RFC 3339, or duration (for example 30m)")
                QQC2.ToolTip.visible: hovered
            }
            QQC2.Label {
                visible: root.logsModel && root.logsModel.groupSelection
                text: I18n.i18nd("tuxstack", "Container:")
            }
            QQC2.ComboBox {
                id: memberSelector
                Layout.fillWidth: visible
                visible: root.logsModel && root.logsModel.groupSelection
                model: root.logsModel ? root.logsModel.memberModel : []
                textRole: "label"
                valueRole: "id"
                onActivated: if (root.logsModel) root.logsModel.setMemberFilter(currentValue)

                function syncCurrentMember() {
                    const selected = root.logsModel ? String(root.logsModel.memberFilterId) : ""
                    for (let index = 0; index < count; ++index) {
                        if (String(valueAt(index)) === selected) {
                            currentIndex = index
                            return
                        }
                    }
                    currentIndex = 0
                }

                onModelChanged: syncCurrentMember()
                Connections {
                    target: root.logsModel
                    function onMemberFilterIdChanged() { memberSelector.syncCurrentMember() }
                    function onMemberModelChanged() { memberSelector.syncCurrentMember() }
                }
            }
            Item {
                Layout.fillWidth: !memberSelector.visible
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.logsModel && root.logsModel.validationError.length > 0
            type: Kirigami.MessageType.Error
            text: root.logsModel ? root.logsModel.validationError : ""
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.logsModel && root.logsModel.discarded
            type: Kirigami.MessageType.Warning
            text: root.logsModel ? root.logsModel.discardedMessage : ""
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.logsModel && root.logsModel.status === "error"
            type: Kirigami.MessageType.Error
            text: root.logsModel ? root.logsModel.errorMessage : ""
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: Kirigami.Theme.backgroundColor
            border.color: Kirigami.Theme.disabledTextColor
            border.width: 1

            ListView {
                id: logList
                anchors.fill: parent
                anchors.margins: 1
                clip: true
                model: root.logsModel
                boundsBehavior: Flickable.StopAtBounds
                cacheBuffer: height
                spacing: 0

                onContentYChanged: {
                    const distance = contentHeight - (contentY + height)
                    root.autoScroll = distance <= Kirigami.Units.gridUnit * 2
                }
                onCountChanged: {
                    if (root.autoScroll && root.logsModel && root.logsModel.follow)
                        Qt.callLater(positionViewAtEnd)
                }

                delegate: Rectangle {
                    id: logRow
                    required property string stream
                    required property string displayText
                    width: logList.width
                    height: logText.implicitHeight + Kirigami.Units.smallSpacing
                    color: stream === "stderr"
                           ? Qt.rgba(Kirigami.Theme.negativeTextColor.r,
                                     Kirigami.Theme.negativeTextColor.g,
                                     Kirigami.Theme.negativeTextColor.b, 0.07)
                           : (index % 2 === 0
                              ? Qt.rgba(Kirigami.Theme.textColor.r,
                                        Kirigami.Theme.textColor.g,
                                        Kirigami.Theme.textColor.b, 0.025)
                              : "transparent")

                    QQC2.TextArea {
                        id: logText
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        leftPadding: Kirigami.Units.smallSpacing
                        rightPadding: Kirigami.Units.smallSpacing
                        topPadding: 0
                        bottomPadding: 0
                        text: logRow.displayText
                        textFormat: TextEdit.PlainText
                        readOnly: true
                        selectByMouse: true
                        persistentSelection: true
                        font.family: "monospace"
                        color: Kirigami.Theme.textColor
                        background: null
                        wrapMode: root.logsModel && root.logsModel.wrap
                                  ? TextEdit.WrapAnywhere : TextEdit.NoWrap
                    }
                }

                QQC2.ScrollBar.vertical: QQC2.ScrollBar { }
                QQC2.ScrollBar.horizontal: QQC2.ScrollBar {
                    policy: root.logsModel && root.logsModel.wrap
                            ? QQC2.ScrollBar.AlwaysOff : QQC2.ScrollBar.AsNeeded
                }
            }

            QQC2.Button {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: Kirigami.Units.largeSpacing
                visible: !root.autoScroll && root.logsModel && root.logsModel.follow
                text: I18n.i18nd("tuxstack", "Jump to bottom")
                icon.name: "go-bottom"
                onClicked: {
                    root.autoScroll = true
                    logList.positionViewAtEnd()
                }
            }

            QQC2.BusyIndicator {
                anchors.centerIn: parent
                visible: root.logsModel && root.logsModel.status === "streaming"
                         && root.logsModel.count === 0
                running: visible
            }
        }
    }

    QQC2.TextArea {
        id: clipboardBuffer
        visible: false
    }

    FileDialog {
        id: saveDialog
        title: I18n.i18nd("tuxstack", "Save Current Log Viewport")
        fileMode: FileDialog.SaveFile
        defaultSuffix: "log"
        nameFilters: [I18n.i18nd("tuxstack", "Log files (*.log)"),
                      I18n.i18nd("tuxstack", "Text files (*.txt)"),
                      I18n.i18nd("tuxstack", "All files (*)")]
        onAccepted: if (root.logsModel) root.logsModel.saveViewport(selectedFile.toString())
    }
}
