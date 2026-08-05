import QtQuick
import QtQuick.Dialogs
import org.tuxstack.app

FileDialog {
    id: root

    property string containerPath: ""
    signal saveRequested(string containerPath, string destination)

    title: I18n.i18nd("tuxstack", "Save Container File As")
    fileMode: FileDialog.SaveFile
    acceptLabel: I18n.i18nd("tuxstack", "Save")

    onAccepted: {
        if (root.containerPath.length > 0)
            root.saveRequested(root.containerPath, selectedFile.toString())
    }
}
