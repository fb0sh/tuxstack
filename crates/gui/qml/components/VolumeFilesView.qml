import QtQuick
import org.tuxstack.app

/** Volume Files wrapper over the shared local FUSE browser. */
LocalFuseFilesView {
    id: root
    property var volumesModel: null
}
