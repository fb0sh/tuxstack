import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

/**
 * Settings page: read-only view of the effective configuration.
 */
Kirigami.Page {
    id: root

    property string dockerHost: ""
    property string configWarning: ""

    title: i18nd("tuxstack", "Settings")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.mediumSpacing

        ErrorBanner {
            Layout.fillWidth: true
            textMessage: root.configWarning
        }

        Kirigami.Heading {
            level: 3
            text: i18nd("tuxstack", "Docker connection")
        }

        RowLayout {
            Layout.fillWidth: true
            QQC2.Label {
                text: i18nd("tuxstack", "Host:")
                Layout.preferredWidth: Kirigami.Units.gridUnit * 8
            }
            QQC2.Label {
                text: root.dockerHost.length > 0 ? root.dockerHost : i18nd("tuxstack", "default")
                font.family: "monospace"
                Layout.fillWidth: true
            }
        }

        Kirigami.Heading {
            level: 3
            text: i18nd("tuxstack", "Configuration file")
            Layout.topMargin: Kirigami.Units.largeSpacing
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: i18nd("tuxstack",
                "Settings are read from the TOML file at\n%1\n\n\
Keys: [docker] host, connect_timeout_seconds, operation_timeout_seconds; \
[ui] auto_refresh_seconds, stats_refresh_seconds, log_line_limit, confirm_remove; \
[logging] level.\n\n\
The theme always follows the system (Breeze Light / Breeze Dark).",
                "~/.config/tuxstack/config.toml")
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.disabledTextColor
        }

        Item {
            Layout.fillHeight: true
            Layout.fillWidth: true
        }
    }
}
