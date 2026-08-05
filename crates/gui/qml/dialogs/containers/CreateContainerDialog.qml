pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
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
    property string environmentImportError: ""
    property string environmentImportMessage: ""
    property string pendingPullRequest: ""
    property string pendingPullImage: ""

    readonly property bool creating: root.containersModel && root.containersModel.creating
    readonly property string localValidationError: validationError()
    readonly property bool valid: root.localValidationError.length === 0

    title: I18n.i18nd("tuxstack", "Create Container")
    preferredWidth: Kirigami.Units.gridUnit * 44
    preferredHeight: Kirigami.Units.gridUnit * 34
    closePolicy: root.creating ? QQC2.Popup.NoAutoClose : QQC2.Popup.CloseOnEscape

    function prepare() {
        root.submitted = false
        if (root.containersModel)
            root.containersModel.clearCreateError()
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
        root.environmentImportError = ""
        root.environmentImportMessage = ""
        root.pendingPullRequest = ""
        root.pendingPullImage = ""
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
        const raw = String(text).trim()
        return raw.length === 0 ? [] : raw.split(",").map(value => value.trim())
    }

    function parseLabels() {
        const labels = ({})
        for (const raw of labelsField.text.split(/\r?\n/)) {
            const line = raw.trim()
            if (line.length === 0)
                continue
            const equals = line.indexOf("=")
            const key = line.substring(0, equals).trim()
            labels[key] = line.substring(equals + 1)
        }
        return labels
    }

    function validContainerPath(value) {
        const path = String(value).trim()
        if (path === "/")
            return true
        if (path.length === 0 || path[0] !== "/" || path.indexOf("\u0000") >= 0)
            return false
        const components = path.substring(1).split("/")
        return components.every(component => component.length > 0 && component !== "." && component !== "..")
    }

    function validIpv4(value) {
        const parts = String(value).split(".")
        if (parts.length !== 4)
            return false
        return parts.every(part => /^(0|[1-9][0-9]{0,2})$/.test(part)
                           && Number(part) <= 255)
    }

    function validIpv6(value) {
        const address = String(value)
        if (address.length === 0 || address.indexOf("%") >= 0 || !/^[0-9A-Fa-f:.]+$/.test(address))
            return false
        const compressed = address.indexOf("::")
        if (compressed >= 0 && address.indexOf("::", compressed + 2) >= 0)
            return false
        const halves = compressed >= 0 ? address.split("::") : [address]
        const left = halves[0].length === 0 ? [] : halves[0].split(":")
        const right = compressed < 0 || halves[1].length === 0 ? [] : halves[1].split(":")
        const components = left.concat(right)
        let units = 0
        for (let index = 0; index < components.length; ++index) {
            const component = components[index]
            if (component.indexOf(".") >= 0) {
                if (index !== components.length - 1 || !validIpv4(component))
                    return false
                units += 2
            } else {
                if (!/^[0-9A-Fa-f]{1,4}$/.test(component))
                    return false
                units += 1
            }
        }
        return compressed >= 0 ? units < 8 : units === 8
    }

    function validIpAddress(value) {
        const address = String(value).trim()
        return validIpv4(address) || validIpv6(address)
    }

    function positiveDecimal(value) {
        const text = String(value).trim()
        if (!/^(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)$/.test(text))
            return null
        const number = Number(text)
        return Number.isFinite(number) && number > 0 ? number : null
    }

    function unsignedInteger(value) {
        const text = String(value).trim()
        if (!/^[0-9]+$/.test(text))
            return null
        const number = Number(text)
        return Number.isSafeInteger(number) ? number : null
    }

    function portValidationError() {
        const seen = ({})
        for (let index = 0; index < portRows.count; ++index) {
            const row = portRows.get(index)
            const containerText = String(row.containerPort).trim()
            const containerPort = unsignedInteger(containerText)
            if (containerPort === null || containerPort < 1 || containerPort > 65535)
                return I18n.i18nd("tuxstack", "Container ports must be whole numbers from 1 to 65535.")
            const hostIp = String(row.hostIp).trim()
            if (hostIp.length > 0 && !validIpAddress(hostIp))
                return I18n.i18nd("tuxstack", "Published host IP addresses must be valid IPv4 or IPv6 addresses.")
            const protocol = String(row.protocol)
            if (["Tcp", "Udp", "Sctp"].indexOf(protocol) < 0)
                return I18n.i18nd("tuxstack", "Published port protocols must be TCP, UDP, or SCTP.")
            const hostText = String(row.hostPort).trim()
            if (hostText.length > 0) {
                const hostPort = unsignedInteger(hostText)
                if (hostPort === null || hostPort < 1 || hostPort > 65535)
                    return I18n.i18nd("tuxstack", "Host ports must be whole numbers from 1 to 65535.")
                const key = "binding:" + hostIp + "\u0000" + hostPort + "\u0000" + protocol
                if (seen[key])
                    return I18n.i18nd("tuxstack", "Published host IP, port, and protocol combinations must be unique.")
                seen[key] = true
            }
        }
        return ""
    }

    function mountValidationError() {
        const seen = ({})
        for (let index = 0; index < mountRows.count; ++index) {
            const row = mountRows.get(index)
            const destination = String(row.destination).trim()
            if (!validContainerPath(destination))
                return I18n.i18nd("tuxstack", "Mount destinations must be absolute container paths without empty, '.' or '..' components.")
            const destinationKey = "destination:" + destination
            if (seen[destinationKey])
                return I18n.i18nd("tuxstack", "Mount destinations must be unique.")
            seen[destinationKey] = true
            const source = String(row.source).trim()
            if (row.kind === "Volume" && (source.length === 0 || source.indexOf("\u0000") >= 0))
                return I18n.i18nd("tuxstack", "Volume mounts require a nonempty source name without NUL.")
            if (row.kind === "Bind" && !validContainerPath(source))
                return I18n.i18nd("tuxstack", "Bind mount sources must be absolute host paths without empty, '.' or '..' components.")
            if (["Volume", "Bind", "Tmpfs"].indexOf(String(row.kind)) < 0)
                return I18n.i18nd("tuxstack", "Mount types must be volume, bind, or tmpfs.")
            if (row.kind === "Tmpfs") {
                const sizeText = String(row.sizeMiB).trim()
                if (sizeText.length > 0) {
                    const sizeMiB = positiveDecimal(sizeText)
                    const sizeBytes = sizeMiB === null ? 0 : Math.round(sizeMiB * 1024 * 1024)
                    if (!Number.isSafeInteger(sizeBytes) || sizeBytes < 1)
                        return I18n.i18nd("tuxstack", "Tmpfs size must be a positive MiB value in the supported numeric range.")
                }
                const modeText = String(row.mode).trim()
                if (modeText.length > 0
                        && (!/^0?[0-7]{1,4}$/.test(modeText) || parseInt(modeText, 8) > 0o7777))
                    return I18n.i18nd("tuxstack", "Tmpfs mode must be an octal value from 0 to 07777.")
            }
        }
        return ""
    }

    function environmentValidationError() {
        const seen = ({})
        for (let index = 0; index < environmentRows.count; ++index) {
            const row = environmentRows.get(index)
            const key = String(row.key).trim()
            if (key.length === 0 || key.indexOf("=") >= 0 || key.indexOf("\u0000") >= 0)
                return I18n.i18nd("tuxstack", "Environment keys must be nonempty and cannot contain '=' or NUL.")
            if (String(row.value).indexOf("\u0000") >= 0)
                return I18n.i18nd("tuxstack", "Environment values cannot contain NUL.")
            const seenKey = "environment:" + key
            if (seen[seenKey])
                return I18n.i18nd("tuxstack", "Environment keys must be unique.")
            seen[seenKey] = true
        }
        return ""
    }

    function applyImportedEnvironment(entries, message) {
        const existing = ({})
        for (let index = 0; index < environmentRows.count; ++index)
            existing[String(environmentRows.get(index).key)] = true
        for (const entry of entries) {
            const key = String(entry.key)
            if (existing[key]) {
                root.environmentImportMessage = ""
                root.environmentImportError = I18n.i18nd(
                    "tuxstack",
                    "The .env file contains “%1”, which is already present. Nothing was imported.",
                    key)
                return
            }
            existing[key] = true
        }
        for (const entry of entries)
            environmentRows.append({ key: String(entry.key), value: String(entry.value) })
        root.environmentImportError = ""
        root.environmentImportMessage = String(message || I18n.i18nd(
            "tuxstack", "Imported %1 environment variables.", entries.length))
    }

    function networkValidationError() {
        const seenNetworks = ({})
        for (let index = 0; index < networkRows.count; ++index) {
            const row = networkRows.get(index)
            const name = String(row.name).trim()
            if (name.length === 0 || name.indexOf("\u0000") >= 0)
                return I18n.i18nd("tuxstack", "Network names must be nonempty and cannot contain NUL.")
            const networkKey = "network:" + name
            if (seenNetworks[networkKey])
                return I18n.i18nd("tuxstack", "Network names must be unique.")
            seenNetworks[networkKey] = true
            const aliases = parseAliases(row.aliases)
            const seenAliases = ({})
            for (const alias of aliases) {
                if (alias.length === 0 || alias.indexOf("\u0000") >= 0)
                    return I18n.i18nd("tuxstack", "Network aliases must be nonempty and cannot contain NUL.")
                const aliasKey = "alias:" + alias
                if (seenAliases[aliasKey])
                    return I18n.i18nd("tuxstack", "Aliases on the same network must be unique.")
                seenAliases[aliasKey] = true
            }
            const ipv4 = String(row.ipv4).trim()
            if (ipv4.length > 0 && !validIpv4(ipv4))
                return I18n.i18nd("tuxstack", "Network IPv4 addresses must use valid IPv4 syntax.")
            const ipv6 = String(row.ipv6).trim()
            if (ipv6.length > 0 && !validIpv6(ipv6))
                return I18n.i18nd("tuxstack", "Network IPv6 addresses must use valid IPv6 syntax.")
        }
        return ""
    }

    function resourceValidationError() {
        const cpuText = cpuField.text.trim()
        if (cpuText.length > 0) {
            const cpu = positiveDecimal(cpuText)
            const millis = cpu === null ? 0 : Math.round(cpu * 1000)
            if (!Number.isSafeInteger(millis) || millis < 1 || millis > 4294967295)
                return I18n.i18nd("tuxstack", "CPU limit must be positive and fit the supported range.")
        }
        const memoryText = memoryField.text.trim()
        if (memoryText.length > 0) {
            const memory = positiveDecimal(memoryText)
            const bytes = memory === null ? 0 : Math.round(memory * 1024 * 1024)
            if (!Number.isSafeInteger(bytes) || bytes < 1)
                return I18n.i18nd("tuxstack", "Memory limit must be a positive MiB value in the supported range.")
        }
        const pidsText = pidsField.text.trim()
        if (pidsText.length > 0) {
            const pids = unsignedInteger(pidsText)
            if (pids === null || pids < 1)
                return I18n.i18nd("tuxstack", "PIDs limit must be a positive whole number.")
        }
        return ""
    }

    function restartValidationError() {
        if (autoRemoveCheck.checked && restartPolicy.currentIndex !== 0)
            return I18n.i18nd("tuxstack", "Auto remove cannot be combined with a restart policy.")
        if (restartPolicy.currentIndex === 3 && retryField.text.trim().length > 0) {
            const retries = unsignedInteger(retryField.text)
            if (retries === null)
                return I18n.i18nd("tuxstack", "Maximum retries must be a nonnegative whole number.")
        }
        return ""
    }

    function labelValidationError() {
        const seen = ({})
        for (const raw of labelsField.text.split(/\r?\n/)) {
            const line = raw.trim()
            if (line.length === 0)
                continue
            const equals = line.indexOf("=")
            const key = equals < 0 ? "" : line.substring(0, equals).trim()
            const value = equals < 0 ? "" : line.substring(equals + 1)
            if (equals < 0)
                return I18n.i18nd("tuxstack", "Labels must use key=value, one per line.")
            if (key.length === 0 || key.indexOf("\u0000") >= 0)
                return I18n.i18nd("tuxstack", "Label keys must be nonempty and cannot contain NUL.")
            if (value.indexOf("\u0000") >= 0)
                return I18n.i18nd("tuxstack", "Label values cannot contain NUL.")
            const seenKey = "label:" + key
            if (seen[seenKey])
                return I18n.i18nd("tuxstack", "Label keys must be unique.")
            seen[seenKey] = true
        }
        return ""
    }

    function validationError() {
        if (imageBox.editText.trim().length === 0)
            return I18n.i18nd("tuxstack", "An image reference is required.")
        const name = nameField.text.trim()
        if (name.length > 0 && !/^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(name))
            return I18n.i18nd("tuxstack", "Container name must start with a letter or digit and contain only letters, digits, '_', '.', or '-'.")
        if (workingDirectory.text.trim().length > 0 && !validContainerPath(workingDirectory.text))
            return I18n.i18nd("tuxstack", "Working directory must be an absolute container path without empty, '.' or '..' components.")
        return portValidationError()
                || mountValidationError()
                || environmentValidationError()
                || networkValidationError()
                || resourceValidationError()
                || restartValidationError()
                || labelValidationError()
    }

    function buildRequest() {
        const ports = []
        for (let index = 0; index < portRows.count; ++index) {
            const row = portRows.get(index)
            ports.push({
                container_port: Number(String(row.containerPort).trim()),
                protocol: String(row.protocol).toLowerCase(),
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
        const policyNames = ["no", "always", "unless_stopped", "on_failure"]
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
        root.containersModel.createContainer(JSON.stringify(buildRequest()))
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
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Label { Layout.fillWidth: true; text: I18n.i18nd("tuxstack", "Environment Variables"); font.bold: true }
                    QQC2.Button {
                        visible: root.containersModel && root.containersModel.localEndpoint
                        text: I18n.i18nd("tuxstack", "Import .env")
                        icon.name: "document-import"
                        onClicked: environmentFileDialog.open()
                    }
                    QQC2.Button { text: I18n.i18nd("tuxstack", "Add Variable"); onClicked: environmentRows.append({key: "", value: ""}) }
                }
                Kirigami.InlineMessage {
                    Layout.fillWidth: true
                    visible: root.environmentImportError.length > 0
                    type: Kirigami.MessageType.Error
                    text: root.environmentImportError
                }
                Kirigami.InlineMessage {
                    Layout.fillWidth: true
                    visible: root.environmentImportError.length === 0
                             && root.environmentImportMessage.length > 0
                    type: Kirigami.MessageType.Positive
                    text: root.environmentImportMessage
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    visible: root.containersModel && !root.containersModel.localEndpoint
                    text: I18n.i18nd("tuxstack", ".env import is available only for a local Docker endpoint.")
                    color: Kirigami.Theme.disabledTextColor
                    wrapMode: Text.Wrap
                }
                Repeater {
                    model: environmentRows
                    delegate: RowLayout {
                        required property int index; required property string key; required property string value
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: "KEY"; text: parent.key; onTextEdited: environmentRows.setProperty(parent.index, "key", text) }
                        QQC2.TextField { Layout.fillWidth: true; placeholderText: I18n.i18nd("tuxstack", "Value"); text: parent.value; echoMode: TextInput.PasswordEchoOnEdit; onTextEdited: environmentRows.setProperty(parent.index, "value", text) }
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
            visible: text.length > 0
            type: Kirigami.MessageType.Error
            text: root.containersModel && root.containersModel.createErrorMessage.length > 0
                  ? root.containersModel.createErrorMessage
                  : root.localValidationError
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button { text: I18n.i18nd("tuxstack", "Cancel"); enabled: !root.creating; QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole; onClicked: root.close() }
        QQC2.Button { text: root.creating ? I18n.i18nd("tuxstack", "Creating…") : I18n.i18nd("tuxstack", "Create Container"); icon.name: "list-add"; enabled: root.valid && !root.creating; QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole; onClicked: root.submit() }
    }

    FileDialog {
        id: environmentFileDialog
        title: I18n.i18nd("tuxstack", "Import Environment File")
        fileMode: FileDialog.OpenFile
        nameFilters: [I18n.i18nd("tuxstack", "Environment files (.env)"),
                      I18n.i18nd("tuxstack", "Text files (*.env *.txt)"),
                      I18n.i18nd("tuxstack", "All files (*)")]
        onAccepted: {
            root.environmentImportError = ""
            root.environmentImportMessage = ""
            if (root.containersModel)
                root.containersModel.importEnvironmentFile(String(selectedFile))
        }
    }

    Kirigami.Dialog {
        id: pullConfirmation
        title: I18n.i18nd("tuxstack", "Pull Missing Image")
        preferredWidth: Kirigami.Units.gridUnit * 30
        closePolicy: root.creating ? QQC2.Popup.NoAutoClose : QQC2.Popup.CloseOnEscape

        ColumnLayout {
            spacing: Kirigami.Units.mediumSpacing
            Kirigami.Heading {
                Layout.fillWidth: true
                level: 3
                text: I18n.i18nd("tuxstack", "Image “%1” is not available locally.", root.pendingPullImage)
                wrapMode: Text.WrapAnywhere
            }
            QQC2.Label {
                Layout.fillWidth: true
                text: I18n.i18nd("tuxstack", "Pull this image from its registry, then create the container?")
                wrapMode: Text.Wrap
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: root.containersModel && root.containersModel.createErrorMessage.length > 0
                type: Kirigami.MessageType.Error
                text: root.containersModel ? root.containersModel.createErrorMessage : ""
            }
        }

        footer: QQC2.DialogButtonBox {
            QQC2.Button {
                text: I18n.i18nd("tuxstack", "Cancel")
                enabled: !root.creating
                QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
                onClicked: pullConfirmation.close()
            }
            QQC2.Button {
                text: root.creating
                      ? I18n.i18nd("tuxstack", "Pulling…")
                      : I18n.i18nd("tuxstack", "Pull and Create")
                icon.name: "download"
                enabled: !root.creating && root.pendingPullRequest.length > 0
                QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
                onClicked: root.containersModel.confirmPullAndCreate(root.pendingPullRequest)
            }
        }
    }

    Connections {
        target: root.containersModel
        ignoreUnknownSignals: true

        function onImagePullRequired(requestJson, imageReference) {
            root.pendingPullRequest = String(requestJson)
            root.pendingPullImage = String(imageReference)
            pullConfirmation.open()
        }
        function onEnvironmentFileImported(entries, message) {
            root.applyImportedEnvironment(entries, message)
        }
        function onEnvironmentFileImportFailed(message) {
            root.environmentImportMessage = ""
            root.environmentImportError = String(message)
        }
        function onContainerCreated() {
            pullConfirmation.close()
            root.pendingPullRequest = ""
            root.pendingPullImage = ""
        }
    }
}
