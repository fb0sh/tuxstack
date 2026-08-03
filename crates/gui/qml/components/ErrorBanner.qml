import QtQuick
import org.kde.kirigami as Kirigami

/**
 * Error banner using Kirigami.InlineMessage.
 */
Kirigami.InlineMessage {
    id: root

    property string textMessage: ""
    type: Kirigami.MessageType.Error
    position: Kirigami.InlineMessage.Header
    showCloseButton: true
    text: textMessage
    visible: textMessage.length > 0
}
