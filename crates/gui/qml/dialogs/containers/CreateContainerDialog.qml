pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var containersModel: null
    property var imagesModel: null
    property var networksModel: null
    property var volumesModel: null
    property bool submitted: false

    readonly property bool creating: root.containersModel && root.containersModel.creating
    readonly property bool valid: imageBox.editText.trim().length > 0
                                  && nameField.acceptableInput
                                  && mountRowsValid()
                                  && portRowsValid()
                                  && environmentRowsValid()

    title: I18n.i18nd("tuxstack", "Create Container")
    preferredWidth: Kirigami.Units.gridUnit * 44
    preferredHeight: Kirigami.Units.gridUnit * 34
    closePolicy: root.creating ? QQC2.Popup.NoAutoClose : QQC2.Popup.CloseOnEscape

    function prepare() {
        root.submitted = false
        nameField.clear()
        imageBox.editText = ""
        createAndStart.checked = true
        entrypointField.clear()
        commandField.clear()
        workingDirectory.clear()
        userField.clear()
        ttyCheck.checked = false
        stdinCheck.checked = false
        portRows.clear()
        mountRows.clear()
        environmentRows.clear()
        networkRows.clear()
        cpuField.clear()
        memoryField.clear()
        pidsField.clear()
        restartPolicy.currentIndex = 0
        retryField.clear()
        labelsField.clear()
        platformField.clear()
        hostnameField.clear()
        domainField.clear()
        readOnlyCheck.checked = false
        privilegedCheck.checked = false
        autoRemoveCheck.checked = false
        tabs.currentIndex = 0
        open()
    }

    function optionalText(value) {
        const text = String(value).trim()
        return text.length > 0 ? text : null
    }

    function optionalNumber(value) {
        const text = String(value).trim()
        if (text.length === 0)
            return null
        const number = Number(text)
        return Number.isFinite(number) ? number : null
    }

    function argv(text) {
        // One line is exactly one argument. Spaces are preserved and never
        // interpreted using shell syntax.
        return String(text).split(/\r?\n/).filter(line => line.length > 0)
    }

    function parseAliases(text) {
        return String(text).split(",").map(value => value.trim()).filter(value => value.length > 0)
    }

    function parseLabels() {
        const labels = ({})
        for (const raw of labelsField.text.split(/\r?\n/)) {
            const line = raw.trim()
            if (line.length === 0)
                continue
            const equals = line.indexOf("=")
            if (equals <= 0)
                throw new Error(I18n.i18nd("tuxstack", "Labels must use key=value, one per line."))
            labels[line.substring(0, equals).trim()] = line.substring(equals + 1)
        }
        return labels
    }

    function portRowsValid() {
        const seen = ({})
        for (let index = 0; index < portRows.count; ++index) {
            const row = portRows.get(index)
            const containerPort = Number(row.containerPort)
            if (!Number.isInteger(containerPort) || containerPort < 1 || containerPort > 65535)
                return false
            if (String(row.hostPort).length > 0) {
                const hostPort = Number(row.hostPort)
                if (!Number.isInteger(hostPort) || hostPort < 1 || hostPort > 65535)
                    return false
                const key = String(row.hostIp) + ":" + hostPort + "/" + String(row.protocol)
                if (seen[key]) return false
                seen[key] = true
            }
        }
        return true
    }

    function mountRowsValid() {
        const seen = ({})
        for (let index = 0; index < mountRows.count; ++index) {
            const row = mountRows.get(index)
            const destination = String(row.destination).trim()
            if (destination.length === 0 || destination[0] !== "/" || seen[destination])
                return false
            seen[destination] = true
            if (row.kind !== "Tmpfs" && String(row.source).trim().length === 0)
                return false
            if (row.kind === "Bind" && String(row.source).trim()[0] !== "/")
                return false
        }
        return true
    }

    function environmentRowsValid() {
        const seen = ({})
        for (let index = 0; index < environmentRows.count; ++index) {
            const key = String(environmentRows.get(index).key).trim()
            if (key.length === 0 || key.indexOf("=") >= 0 || seen[key])
                return false
            seen[key] = true
        }
        return true
    }

    function buildRequest() {
        const ports = []
        for (let index = 0; index < portRows.count; ++index) {
            const row = portRows.get(index)
            ports.push({
                container_port: Number(row.containerPort),
                protocol: String(row.protocol),
                host_ip: optionalText(row.hostIp),
                host_port: optionalNumber(row.hostPort)
            })
        }
        const mounts = []
        for (let index = 0; index < mountRows.count; ++index) {
            const row = mountRows.get(index)
            if (row.kind === "Volume") {
                mounts.push({ Volume: { source: String(row.source).trim(), destination: String(row.destination).trim(), read_only: Boolean(row.readOnly) } })
            } else if (row.kind === "Bind") {
                mounts.push({ Bind: { source: String(row.source).trim(), destination: String(row.destination).trim(), read_only: Boolean(row.readOnly), propagation: optionalText(row.propagation) } })
            } else {
                const size = optionalNumber(row.sizeMiB)
                const modeText = String(row.mode).trim()
                mounts.push({ Tmpfs: { destination: String(row.destination).trim(), size_bytes: size === null ? null : Math.round(size * 1024 * 1024), mode: modeText.length === 0 ? null : parseInt(modeText, 8) } })
            }
        }
        const environment = []
        for (let index = 0; index < environmentRows.count; ++index) {
            const row = environmentRows.get(index)
            environment.push({ key: String(row.key).trim(), value: String(row.value) })
        }
        const networks = []
        for (let index = 0; index < networkRows.count; ++index) {
            const row = networkRows.get(index)
            networks.push({ name: String(row.name).trim(), aliases: parseAliases(row.aliases), ipv4_address: optionalText(row.ipv4), ipv6_address: optionalText(row.ipv6) })
        }
        const policyNames = ["No", "Always", "UnlessStopped", "OnFailure"]
        const cpu = optionalNumber(cpuField.text)
        const memory = optionalNumber(memoryField.text)
        return {
            name: optionalText(nameField.text),
            image: imageBox.editText.trim(),
            platform: optionalText(platformField.text),
            hostname: optionalText(hostnameField.text),
            domain_name: optionalText(domainField.text),
            entrypoint: argv(entrypointField.text),
            command: argv(commandField.text),
            working_directory: optionalText(workingDirectory.text),
            user: optionalText(userField.text),
            tty: ttyCheck.checked,
            open_stdin: stdinCheck.checked,
            ports: ports,
            mounts: mounts,
            environment: environment,
            networks: networks,
            resources: {
                cpu_cores_millis: cpu === null ? null : Math.round(cpu * 1000),
                memory_bytes: memory === null ? null : Math.round(memory * 1024 * 1024),
                pids_limit: optionalNumber(pidsField.text)
            },
            restart_policy: {
                name: policyNames[restartPolicy.currentIndex],
                maximum_retry_count: restartPolicy.currentIndex === 3 ? optionalNumber(retryField.text) : null
            },
            labels: parseLabels(),
            read_only_rootfs: readOnlyCheck.checked,
            privileged: privilegedCheck.checked,
            auto_remove: autoRemoveCheck.checked,
            create_and_start: createAndStart.checked
        }
    }

    function submit() {
        if (!root.valid || root.creating || !root.containersModel)
            return
        root.submitted = true
        try {
            root.containersModel.createContainer(JSON.stringify(buildRequest()))
        } catch (error) {
            localError.text = String(error)
        }
    }

    ListModel { id: portRows }
    ListModel { id: mountRows }
    ListModel { id: environmentRows }
    ListModel { id: networkRows }

    ColumnLayout {
        spacing: 0

        QQC2.TabBar {
            id: tabs
            Layout.fillWidth: true
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Basic") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Command") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Ports") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Mounts") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Environment") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Networks") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Resources") }
            QQC2.TabButton { text: I18n.i18nd("tuxstack", "Advanced") }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: tabs.currentIndex

            GridLayout {
                columns: 2
                QQC2.Label { text: I18n.i18nd("tuxstack", "Container Name") }
                QQC2.TextField { id: nameField; Layout.fillWidth: true; validator: RegularExpressionValidator { regularExpression: /(|[A-Za-z0-9][A-Za-z0-9_.-]*)/ } }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Image") }
                QQC2.ComboBox { id: imageBox; Layout.fillWidth: true; editable: true; model: root.imagesModel; textRole: "displayName" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Action") }
                QQC2.CheckBox { id: createAndStart; text: I18n.i18nd("tuxstack", "Create and Start"); checked: true }
            }

            GridLayout {
                columns: 2
                QQC2.Label { text: I18n.i18nd("tuxstack", "Entrypoint arguments"); Layout.alignment: Qt.AlignTop }
                QQC2.TextArea { id: entrypointField; Layout.fillWidth: true; Layout.preferredHeight: Kirigami.Units.gridUnit * 5; placeholderText: I18n.i18nd("tuxstack", "One argument per line") }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Command arguments"); Layout.alignment: Qt.AlignTop }
                QQC2.TextArea { id: commandField; Layout.fillWidth: true; Layout.preferredHeight: Kirigami.Units.gridUnit * 5; placeholderText: I18n.i18nd("tuxstack", "One argument per line") }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Working Directory") }
                QQC2.TextField { id: workingDirectory; Layout.fillWidth: true; placeholderText: "/" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "User") }
                QQC2.TextField { id: userField; Layout.fillWidth: true }
                Item { }
                RowLayout { QQC2.CheckBox { id: ttyCheck; text: "TTY" } QQC2.CheckBox { id: stdinCheck; text: I18n.i18nd("tuxstack", "Keep stdin open") } }
            }

            ColumnLayout {
                RowLayout { Layout.fillWidth: true; QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Published Ports"); font.bold: true } QQC2.Button { text: I18n.i18nd("tuxstack", "Add Port"); onClicked: portRows.append({containerPort: "", protocol: "Tcp", hostIp: "", hostPort: ""}) } }
                Repeater {
                    model: portRows
                    delegate: RowLayout {
                        required property int index; required property string containerPort; required property string protocol; required property string hostIp; required property string hostPort
                        QQC2.TextField { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; placeholderText: I18n.i18nd("tuxstack", "Container Port"); text: parent.containerPort; onTextEdited: portRows.setProperty(parent.index, "containerPort", text) }
                        QQC2.ComboBox { model: ["Tcp", "Udp", "Sctp"]; currentIndex: model.indexOf(parent.protocol); onActivated: portRows.setProperty(parent.index, "protocol", currentText) }
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Host IP (optional)"); text: parent.hostIp; onTextEdited: portRows.setProperty(parent.index, "hostIp", text) }
                        QQC2.TextField { Layout.preferredWidth: Kirigami.Units.gridUnit * 6; placeholderText: I18n.i18nd("tuxstack", "Host Port"); text: parent.hostPort; onTextEdited: portRows.setProperty(parent.index, "hostPort", text) }
                        QQC2.ToolButton { icon.name: "list-remove"; onClicked: portRows.remove(parent.index) }
                    }
                }
            }

            ColumnLayout {
                RowLayout { Layout.fillWidth: true; QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Mounts"); font.bold: true } QQC2.Button { text: I18n.i18nd("tuxstack", "Add Mount"); onClicked: mountRows.append({kind: "Volume", source: "", destination: "", readOnly: false, propagation: "", sizeMiB: "", mode: ""}) } }
                Repeater {
                    model: mountRows
                    delegate: ColumnLayout {
                        required property int index; required property string kind; required property string source; required property string destination; required property bool readOnly; required property string propagation; required property string sizeMiB; required property string mode
                        RowLayout {
                            QQC2.ComboBox { model: ["Volume", "Bind", "Tmpfs"]; currentIndex: model.indexOf(parent.parent.kind); onActivated: mountRows.setProperty(parent.parent.index, "kind", currentText) }
                            QQC2.ComboBox { visible: parent.parent.kind === "Volume"; editable: true; model: root.volumesModel; textRole: "name"; editText: parent.parent.source; onEditTextChanged: mountRows.setProperty(parent.parent.index, "source", editText) }
                            QQC2.TextField { visible: parent.parent.kind === "Bind"; Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Host path"); text: parent.parent.source; onTextEdited: mountRows.setProperty(parent.parent.index, "source", text) }
                            QQC2.TextField { Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Container destination"); text: parent.parent.destination; onTextEdited: mountRows.setProperty(parent.parent.index, "destination", text) }
                            QQC2.CheckBox { visible: parent.parent.kind !== "Tmpfs"; text: I18n.i18nd("tuxstack", "Read only"); checked: parent.parent.readOnly; onToggled: mountRows.setProperty(parent.parent.index, "readOnly", checked) }
                            QQC2.ToolButton { icon.name: "list-remove"; onClicked: mountRows.remove(parent.parent.index) }
                        }
                        RowLayout {
                            visible: parent.kind === "Tmpfs" || parent.kind === "Bind"
                            QQC2.TextField { visible: parent.parent.kind === "Bind"; placeholderText: I18n.i18nd("tuxstack", "Propagation (optional)"); text: parent.parent.propagation; onTextEdited: mountRows.setProperty(parent.parent.index, "propagation", text) }
                            QQC2.TextField { visible: parent.parent.kind === "Tmpfs"; placeholderText: I18n.i18nd("tuxstack", "Size MiB"); text: parent.parent.sizeMiB; onTextEdited: mountRows.setProperty(parent.parent.index, "sizeMiB", text) }
                            QQC2.TextField { visible: parent.parent.kind === "Tmpfs"; placeholderText: I18n.i18nd("tuxstack", "Mode (e.g. 1777)"); text: parent.parent.mode; onTextEdited: mountRows.setProperty(parent.parent.index, "mode", text) }
                        }
                    }
                }
            }

            ColumnLayout {
                RowLayout { Layout.fillWidth: true; QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Environment Variables"); font.bold: true } QQC2.Button { text: I18n.i18nd("tuxstack", "Add Variable"); onClicked: environmentRows.append({key: "", value: ""}) } }
                Repeater {
                    model: environmentRows
                    delegate: RowLayout {
                        required property int index; required property string key; required property string value
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: "KEY"; text: parent.key; onTextEdited: environmentRows.setProperty(parent.index, "key", text) }
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Value"); text: parent.value; echoMode: QQC2.TextInput.PasswordEchoOnEdit; onTextEdited: environmentRows.setProperty(parent.index, "value", text) }
                        QQC2.ToolButton { icon.name: "list-remove"; onClicked: environmentRows.remove(parent.index) }
                    }
                }
            }

            ColumnLayout {
                RowLayout { Layout.fillWidth: true; QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Networks"); font.bold: true } QQC2.Button { text: I18n.i18nd("tuxstack", "Add Network"); onClicked: networkRows.append({name: "bridge", aliases: "", ipv4: "", ipv6: ""}) } }
                Repeater {
                    model: networkRows
                    delegate: RowLayout {
                        required property int index; required property string name; required property string aliases; required property string ipv4; required property string ipv6
                        QQC2.ComboBox { Layout.fillWidth: true; editable: true; model: root.networksModel; textRole: "name"; editText: parent.name; onEditTextChanged: networkRows.setProperty(parent.index, "name", editText) }
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Aliases (comma separated)"); text: parent.aliases; onTextEdited: networkRows.setProperty(parent.index, "aliases", text) }
                        QQC2.TextField { Layout.preferredWidth: Kirigami.Units.gridUnit * 7; placeholderText: "IPv4"; text: parent.ipv4; onTextEdited: networkRows.setProperty(parent.index, "ipv4", text) }
                        QQC2.TextField { Layout.preferredWidth: Kirigami.Units.gridUnit * 7; placeholderText: "IPv6"; text: parent.ipv6; onTextEdited: networkRows.setProperty(parent.index, "ipv6", text) }
                        QQC2.ToolButton { icon.name: "list-remove"; onClicked: networkRows.remove(parent.index) }
                    }
                }
            }

            GridLayout {
                columns: 2
                QQC2.Label { text: I18n.i18nd("tuxstack", "CPU limit (cores)") }
                QQC2.TextField { id: cpuField; Layout.fillWidth: true; placeholderText: "1.5" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Memory limit (MiB)") }
                QQC2.TextField { id: memoryField; Layout.fillWidth: true; placeholderText: "512" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "PIDs limit") }
                QQC2.TextField { id: pidsField; Layout.fillWidth: true; placeholderText: "256" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Restart Policy") }
                QQC2.ComboBox { id: restartPolicy; Layout.fillWidth: true; model: [I18n.i18nd("tuxstack", "No"), I18n.i18nd("tuxstack", "Always"), I18n.i18nd("tuxstack", "Unless Stopped"), I18n.i18nd("tuxstack", "On Failure")] }
                QQC2.Label { visible: restartPolicy.currentIndex === 3; text: I18n.i18nd("tuxstack", "Maximum retries") }
                QQC2.TextField { id: retryField; visible: restartPolicy.currentIndex === 3; Layout.fillWidth: true }
            }

            GridLayout {
                columns: 2
                QQC2.Label { text: I18n.i18nd("tuxstack", "Platform") }
                QQC2.TextField { id: platformField; Layout.fillWidth: true; placeholderText: "linux/amd64" }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Hostname") }
                QQC2.TextField { id: hostnameField; Layout.fillWidth: true }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Domain Name") }
                QQC2.TextField { id: domainField; Layout.fillWidth: true }
                QQC2.Label { text: I18n.i18nd("tuxstack", "Labels"); Layout.alignment: Qt.AlignTop }
                QQC2.TextArea { id: labelsField; Layout.fillWidth: true; Layout.preferredHeight: Kirigami.Units.gridUnit * 5; placeholderText: I18n.i18nd("tuxstack", "key=value, one per line") }
                Item { }
                RowLayout { QQC2.CheckBox { id: readOnlyCheck; text: I18n.i18nd("tuxstack", "Read-only root filesystem") } QQC2.CheckBox { id: autoRemoveCheck; text: I18n.i18nd("tuxstack", "Auto remove") } QQC2.CheckBox { id: privilegedCheck; text: I18n.i18nd("tuxstack", "Privileged") } }
            }
        }

        Kirigami.InlineMessage {
            id: localError
            Layout.fillWidth: true
            visible: text.length > 0 || (root.containersModel && root.containersModel.createErrorMessage.length > 0)
            type: Kirigami.MessageType.Error
            text: root.containersModel && root.containersModel.createErrorMessage.length > 0 ? root.containersModel.createErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); enabled: !root.creating; QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button { text: root.creating ? I18n.i18nd("tuxstack", "Creating…") : I18n.i18nd("tuxstack", "Create Container"); icon.name: "list-add"; enabled: root.valid && !root.creating; QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole; onClicked: root.submit() }
    }
}
