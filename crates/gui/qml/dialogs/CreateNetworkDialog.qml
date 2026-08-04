import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.tuxstack.app

Kirigami.Dialog {
    id: root

    property var networksModel: null
    property bool submitted: false
    readonly property bool creating: root.networksModel && root.networksModel.creating
    readonly property string nameError: networkName.text.trim().length === 0
                                                && networkName.text.length > 0
                                         ? I18n.i18nd("tuxstack", "Name is required.") : ""
    readonly property string subnetError: root.cidrError(subnetField.text.trim())
    readonly property string gatewayError: root.addressError(gatewayField.text.trim())
    readonly property string addressFamilyError: root.familyError(subnetField.text.trim(),
                                                                  gatewayField.text.trim())
    readonly property string labelsError: root.validateLabels(labelsField.text)
    readonly property bool valid: networkName.text.trim().length > 0
                                  && root.subnetError.length === 0
                                  && root.gatewayError.length === 0
                                  && root.addressFamilyError.length === 0
                                  && root.labelsError.length === 0
                                  && (gatewayField.text.trim().length === 0
                                      || subnetField.text.trim().length > 0)

    title: I18n.i18nd("tuxstack", "Create Network")
    preferredWidth: Kirigami.Units.gridUnit * 30
    closePolicy: root.creating ? QQC2.Popup.NoAutoClose
                               : QQC2.Popup.CloseOnEscape

    function prepare() {
        root.submitted = false
        networkName.clear()
        driverBox.currentIndex = 0
        subnetField.clear()
        gatewayField.clear()
        ipv6Check.checked = false
        internalCheck.checked = false
        attachableCheck.checked = false
        labelsField.clear()
        open()
        networkName.forceActiveFocus()
    }

    function isIpv4Address(value) {
        const parts = value.split(".")
        if (parts.length !== 4)
            return false
        for (let index = 0; index < parts.length; ++index) {
            if (!/^\d{1,3}$/.test(parts[index]))
                return false
            const octet = Number(parts[index])
            if (octet < 0 || octet > 255)
                return false
        }
        return true
    }

    function isIpv6Address(value) {
        if (value.indexOf(":") < 0 || !/^[0-9A-Fa-f:]+$/.test(value))
            return false

        const compressed = value.indexOf("::") >= 0
        if (compressed && value.indexOf("::") !== value.lastIndexOf("::"))
            return false

        const parts = value.split(":")
        let componentCount = 0
        for (let index = 0; index < parts.length; ++index) {
            if (parts[index].length === 0)
                continue
            if (parts[index].length > 4)
                return false
            componentCount += 1
        }
        return compressed ? componentCount < 8 : componentCount === 8
    }

    function addressError(value) {
        if (value.length === 0)
            return ""
        if (root.isIpv4Address(value) || root.isIpv6Address(value))
            return ""
        return I18n.i18nd("tuxstack", "Enter a valid IPv4 or IPv6 address.")
    }

    function cidrError(value) {
        if (value.length === 0)
            return ""
        const slash = value.lastIndexOf("/")
        if (slash <= 0 || slash === value.length - 1)
            return I18n.i18nd("tuxstack", "Enter a subnet in CIDR notation.")
        const address = value.substring(0, slash)
        const prefix = Number(value.substring(slash + 1))
        const ipv4 = root.isIpv4Address(address)
        const ipv6 = root.isIpv6Address(address)
        if ((!ipv4 && !ipv6) || !Number.isInteger(prefix)
                || prefix < 0 || prefix > (ipv4 ? 32 : 128))
            return I18n.i18nd("tuxstack", "Enter a valid subnet in CIDR notation.")
        return ""
    }

    function familyError(subnet, gateway) {
        if (subnet.length === 0 || gateway.length === 0
                || root.cidrError(subnet).length > 0
                || root.addressError(gateway).length > 0)
            return ""
        const subnetAddress = subnet.substring(0, subnet.lastIndexOf("/"))
        if (root.isIpv4Address(subnetAddress) !== root.isIpv4Address(gateway))
            return I18n.i18nd("tuxstack",
                              "Gateway and subnet must use the same address family.")
        return ""
    }

    function validateLabels(value) {
        const lines = value.split(/\r?\n/)
        for (let index = 0; index < lines.length; ++index) {
            const line = lines[index].trim()
            if (line.length === 0)
                continue
            const equals = line.indexOf("=")
            if (equals <= 0)
                return I18n.i18nd("tuxstack",
                                  "Each label must use key=value format (line %1).")
                       .arg(index + 1)
        }
        return ""
    }

    function beginCreate() {
        if (!root.networksModel || !root.valid || root.creating)
            return
        root.submitted = true
        const flags = (ipv6Check.checked ? 1 : 0)
                      | (internalCheck.checked ? 2 : 0)
                      | (attachableCheck.checked ? 4 : 0)
        root.networksModel.createNetwork(networkName.text.trim(),
                                         String(driverBox.currentText),
                                         subnetField.text.trim(),
                                         gatewayField.text.trim(),
                                         flags,
                                         labelsField.text.trim())
    }

    ColumnLayout {
        spacing: Kirigami.Units.mediumSpacing

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Name")
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                QQC2.TextField {
                    id: networkName
                    Layout.fillWidth: true
                    enabled: !root.creating
                    selectByMouse: true
                    placeholderText: I18n.i18nd("tuxstack", "my-network")
                    onAccepted: root.beginCreate()
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    visible: root.nameError.length > 0
                    text: root.nameError
                    color: Kirigami.Theme.negativeTextColor
                    font: Kirigami.Theme.smallFont
                }
            }

            QQC2.Label { text: I18n.i18nd("tuxstack", "Driver") }
            QQC2.ComboBox {
                id: driverBox
                Layout.fillWidth: true
                enabled: !root.creating
                model: ["bridge", "overlay", "macvlan"]
            }

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Subnet")
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                QQC2.TextField {
                    id: subnetField
                    Layout.fillWidth: true
                    enabled: !root.creating
                    selectByMouse: true
                    placeholderText: I18n.i18nd("tuxstack", "172.20.0.0/16")
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    visible: root.subnetError.length > 0
                    text: root.subnetError
                    color: Kirigami.Theme.negativeTextColor
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
            }

            QQC2.Label {
                text: I18n.i18nd("tuxstack", "Gateway")
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                QQC2.TextField {
                    id: gatewayField
                    Layout.fillWidth: true
                    enabled: !root.creating
                    selectByMouse: true
                    placeholderText: I18n.i18nd("tuxstack", "172.20.0.1")
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    visible: root.gatewayError.length > 0
                             || root.addressFamilyError.length > 0
                             || (gatewayField.text.trim().length > 0
                                 && subnetField.text.trim().length === 0)
                    text: root.gatewayError.length > 0
                          ? root.gatewayError
                          : (root.addressFamilyError.length > 0
                             ? root.addressFamilyError
                             : I18n.i18nd("tuxstack", "A gateway requires a subnet."))
                    color: Kirigami.Theme.negativeTextColor
                    font: Kirigami.Theme.smallFont
                    wrapMode: Text.Wrap
                }
            }
        }

        QQC2.CheckBox {
            id: ipv6Check
            text: I18n.i18nd("tuxstack", "Enable IPv6")
            enabled: !root.creating
        }
        QQC2.CheckBox {
            id: internalCheck
            text: I18n.i18nd("tuxstack", "Internal network")
            enabled: !root.creating
        }
        QQC2.CheckBox {
            id: attachableCheck
            text: I18n.i18nd("tuxstack", "Attachable")
            enabled: !root.creating
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: I18n.i18nd("tuxstack", "Labels")
            font.bold: true
        }
        QQC2.TextArea {
            id: labelsField
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 6
            enabled: !root.creating
            selectByMouse: true
            wrapMode: TextEdit.NoWrap
            placeholderText: I18n.i18nd("tuxstack", "com.example.team=platform\nenvironment=development")
            Accessible.description: I18n.i18nd("tuxstack", "One key=value label per line")
        }
        QQC2.Label {
            Layout.fillWidth: true
            visible: root.labelsError.length > 0
            text: root.labelsError
            color: Kirigami.Theme.negativeTextColor
            font: Kirigami.Theme.smallFont
            wrapMode: Text.Wrap
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: root.submitted && root.networksModel
                     && root.networksModel.createErrorMessage.length > 0
            type: Kirigami.MessageType.Error
            text: visible ? root.networksModel.createErrorMessage : ""
        }
    }

    footer: QQC2.DialogButtonBox {
        QQC2.Button {
            text: I18n.i18nd("tuxstack", "Cancel")
            enabled: !root.creating
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.RejectRole
            onClicked: root.close()
        }
        QQC2.Button {
            text: root.creating
                  ? I18n.i18nd("tuxstack", "Creating…")
                  : I18n.i18nd("tuxstack", "Create")
            icon.name: "list-add"
            enabled: root.networksModel && !root.creating && root.valid
            QQC2.DialogButtonBox.buttonRole: QQC2.DialogButtonBox.AcceptRole
            onClicked: root.beginCreate()
        }
    }
}
